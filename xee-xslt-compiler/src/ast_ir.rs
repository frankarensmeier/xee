use ahash::{HashMap, HashMapExt, HashSetExt};
use icu_properties::{maps, GeneralCategory};
use iri_string::types::{IriAbsoluteString, IriReferenceStr};
use xee_name::{Name, Namespaces, FN_NAMESPACE, XS_NAMESPACE};

use std::collections::HashSet;
use std::path::PathBuf;
use xee_interpreter::{
    context::{DecimalFormatSymbols, StaticContext, Variables as StaticVariables},
    error,
    interpreter::{self, instruction::RaisedError},
    sequence::QNameOrString,
};
use xee_ir::{compile_xslt, ir, Bindings, Variables as IrVariables};
use xee_xpath_ast::{ast as xpath_ast, pattern::transform_pattern, span::Spanned};
use xee_xslt_ast::{
    ast,
    error::{AttributeError, ElementError},
    parse_transform_with_static_variables_and_base_dir,
    parse_transform_with_static_variables_and_location,
};
use xot::{
    xmlname::{NameStrInfo, OwnedName},
    Xot,
};

use crate::dynamic_xpath::XsltDynamicXPathEvaluator;
use crate::priority::default_priority;

struct IrConverter<'a> {
    variables: IrVariables,
    static_context: &'a StaticContext,
    overridden_static_context: Option<StaticContext>,
    initial_mode: ast::ApplyTemplatesModeValue,
    xslt_functions: HashMap<(OwnedName, u8), OwnedName>,
    accumulator_declarations: HashMap<OwnedName, PreprocessedDeclaration>,
    referenced_accumulators: HashSet<OwnedName>,
    namespace_aliases: HashMap<String, String>,
    attribute_sets: HashMap<(String, String), Vec<ast::AttributeSet>>,
    named_outputs: HashMap<(String, String), (i64, ast::Output)>,
    character_maps: HashMap<(String, String), (i64, ast::CharacterMap)>,
    resolved_character_maps: HashMap<(String, String), ahash::HashMap<char, String>>,
    active_attribute_sets: Vec<(String, String)>,
    secondary_result_document_depth: usize,
    named_templates_with_absent_context: HashSet<String>,
    template_continuation_available: bool,
    strip_source_document_whitespace: bool,
    number_patterns: Vec<ir::NumberPatternDefinition>,
}

#[derive(Debug, Clone)]
struct PreprocessedDeclaration {
    declaration: ast::Declaration,
    import_precedence: i64,
    module_path: Vec<usize>,
    stylesheet_uri: Option<String>,
}

#[derive(Debug, Clone)]
struct PreprocessedModule {
    declarations: Vec<(ast::Declaration, Option<String>)>,
    module_path: Vec<usize>,
    stylesheet_uri: Option<String>,
}

const ARRAY_NAMESPACE: &str = "http://www.w3.org/2005/xpath-functions/array";
const ERR_NAMESPACE: &str = "http://www.w3.org/2005/xqt-errors";
const MAP_NAMESPACE: &str = "http://www.w3.org/2005/xpath-functions/map";
const MATH_NAMESPACE: &str = "http://www.w3.org/2005/xpath-functions/math";
const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_NAMESPACE: &str = "http://www.w3.org/2000/xmlns/";
const XSI_NAMESPACE: &str = "http://www.w3.org/2001/XMLSchema-instance";
const XSLT_NAMESPACE: &str = "http://www.w3.org/1999/XSL/Transform";

fn is_reserved_stylesheet_namespace(namespace: &str) -> bool {
    matches!(
        namespace,
        XSLT_NAMESPACE
            | FN_NAMESPACE
            | MATH_NAMESPACE
            | MAP_NAMESPACE
            | ARRAY_NAMESPACE
            | XML_NAMESPACE
            | XS_NAMESPACE
            | XSI_NAMESPACE
            | ERR_NAMESPACE
            | XMLNS_NAMESPACE
    )
}

fn is_xsl_initial_template(name: &OwnedName) -> bool {
    name.namespace() == XSLT_NAMESPACE && name.local_name() == "initial-template"
}

fn validate_non_reserved_stylesheet_name(
    name: &OwnedName,
    span: ast::Span,
) -> error::SpannedResult<()> {
    if is_reserved_stylesheet_namespace(name.namespace()) && !is_xsl_initial_template(name) {
        return Err(error::Error::XTSE0080.with_ast_span((span.start..span.end).into()));
    }

    Ok(())
}

#[derive(Debug, Clone, Default)]
struct DecimalFormatAccumulator {
    decimal_separator: Vec<(char, i64)>,
    grouping_separator: Vec<(char, i64)>,
    infinity: Vec<(String, i64)>,
    minus_sign: Vec<(char, i64)>,
    exponent_separator: Vec<(char, i64)>,
    nan: Vec<(String, i64)>,
    percent: Vec<(char, i64)>,
    per_mille: Vec<(char, i64)>,
    zero_digit: Vec<(char, i64)>,
    digit: Vec<(char, i64)>,
    pattern_separator: Vec<(char, i64)>,
}

impl DecimalFormatAccumulator {
    fn merge(
        &mut self,
        declaration: &ast::DecimalFormat,
        import_precedence: i64,
        processor_xslt_version: Option<u8>,
        processor_xpath_version: Option<u8>,
    ) -> error::Result<()> {
        if declaration.exponent_separator.is_some()
            && (processor_xslt_version.unwrap_or(3) < 3
                || processor_xpath_version.unwrap_or(31) < 31)
        {
            return Err(error::Error::XTSE0090);
        }

        push_decimal_format_field(
            &mut self.decimal_separator,
            declaration.decimal_separator,
            import_precedence,
        );
        push_decimal_format_field(
            &mut self.grouping_separator,
            declaration.grouping_separator,
            import_precedence,
        );
        push_decimal_format_field(
            &mut self.infinity,
            declaration.infinity.clone(),
            import_precedence,
        );
        push_decimal_format_field(
            &mut self.minus_sign,
            declaration.minus_sign,
            import_precedence,
        );
        push_decimal_format_field(
            &mut self.exponent_separator,
            declaration.exponent_separator,
            import_precedence,
        );
        push_decimal_format_field(&mut self.nan, declaration.nan.clone(), import_precedence);
        push_decimal_format_field(&mut self.percent, declaration.percent, import_precedence);
        push_decimal_format_field(
            &mut self.per_mille,
            declaration.per_mille,
            import_precedence,
        );
        push_decimal_format_field(
            &mut self.zero_digit,
            declaration.zero_digit,
            import_precedence,
        );
        push_decimal_format_field(&mut self.digit, declaration.digit, import_precedence);
        push_decimal_format_field(
            &mut self.pattern_separator,
            declaration.pattern_separator,
            import_precedence,
        );
        Ok(())
    }

    fn build(&self) -> error::Result<DecimalFormatSymbols> {
        let defaults = DecimalFormatSymbols::default();
        let symbols = DecimalFormatSymbols {
            decimal_separator: resolve_decimal_format_field(
                &self.decimal_separator,
                defaults.decimal_separator,
            )?,
            grouping_separator: resolve_decimal_format_field(
                &self.grouping_separator,
                defaults.grouping_separator,
            )?,
            infinity: resolve_decimal_format_field(&self.infinity, defaults.infinity)?,
            minus_sign: resolve_decimal_format_field(&self.minus_sign, defaults.minus_sign)?,
            exponent_separator: resolve_decimal_format_field(
                &self.exponent_separator,
                defaults.exponent_separator,
            )?,
            nan: resolve_decimal_format_field(&self.nan, defaults.nan)?,
            percent: resolve_decimal_format_field(&self.percent, defaults.percent)?,
            per_mille: resolve_decimal_format_field(&self.per_mille, defaults.per_mille)?,
            zero_digit: resolve_decimal_format_field(&self.zero_digit, defaults.zero_digit)?,
            digit: resolve_decimal_format_field(&self.digit, defaults.digit)?,
            pattern_separator: resolve_decimal_format_field(
                &self.pattern_separator,
                defaults.pattern_separator,
            )?,
        };
        validate_decimal_format_symbols(&symbols)?;
        Ok(symbols)
    }
}

fn push_decimal_format_field<T: Clone>(
    slot: &mut Vec<(T, i64)>,
    value: Option<T>,
    import_precedence: i64,
) {
    let Some(value) = value else {
        return;
    };

    slot.push((value, import_precedence));
}

fn resolve_decimal_format_field<T: Clone + Eq>(
    values: &[(T, i64)],
    default: T,
) -> error::Result<T> {
    let Some(highest_precedence) = values.iter().map(|(_, precedence)| *precedence).max() else {
        return Ok(default);
    };

    let mut candidates = values
        .iter()
        .filter(|(_, precedence)| *precedence == highest_precedence)
        .map(|(value, _)| value);
    let Some(first) = candidates.next() else {
        return Ok(default);
    };
    if candidates.any(|candidate| candidate != first) {
        return Err(error::Error::XTSE1290);
    }

    Ok(first.clone())
}

fn validate_decimal_format_symbols(symbols: &DecimalFormatSymbols) -> error::Result<()> {
    let mut seen = HashSet::new();
    for c in [
        symbols.decimal_separator,
        symbols.grouping_separator,
        symbols.minus_sign,
        symbols.exponent_separator,
        symbols.percent,
        symbols.per_mille,
        symbols.zero_digit,
        symbols.digit,
        symbols.pattern_separator,
    ] {
        if !seen.insert(c) {
            return Err(error::Error::XTSE1300);
        }
    }

    if !is_valid_zero_digit(symbols.zero_digit) {
        return Err(error::Error::XTSE1295);
    }

    Ok(())
}

fn is_valid_zero_digit(zero_digit: char) -> bool {
    let decimal_digits = maps::general_category().get_set_for_value(GeneralCategory::DecimalNumber);
    let decimal_digits = decimal_digits.as_borrowed();
    if !decimal_digits.contains32(zero_digit as u32) {
        return false;
    }

    let zero = zero_digit as u32;
    let has_all_following_digits = (1..=9).all(|offset| {
        char::from_u32(zero + offset)
            .map(|c| decimal_digits.contains32(c as u32))
            .unwrap_or(false)
    });
    if !has_all_following_digits {
        return false;
    }

    zero.checked_sub(1)
        .and_then(char::from_u32)
        .map(|previous| !decimal_digits.contains32(previous as u32))
        .unwrap_or(true)
}

fn augment_static_context_with_decimal_formats(
    declarations: &[PreprocessedDeclaration],
    static_context: &mut StaticContext,
) -> error::SpannedResult<()> {
    let mut default_decimal_format = DecimalFormatAccumulator::default();
    let mut named_decimal_formats = HashMap::new();
    let processor_xslt_version = static_context.processor_xslt_version();
    let processor_xpath_version = static_context.processor_xpath_version();

    for declaration in declarations {
        let ast::Declaration::DecimalFormat(decimal_format) = &declaration.declaration else {
            continue;
        };

        let result = if let Some(name) = &decimal_format.name {
            named_decimal_formats
                .entry(name.clone())
                .or_insert_with(DecimalFormatAccumulator::default)
                .merge(
                    decimal_format,
                    declaration.import_precedence,
                    processor_xslt_version,
                    processor_xpath_version,
                )
        } else {
            default_decimal_format.merge(
                decimal_format,
                declaration.import_precedence,
                processor_xslt_version,
                processor_xpath_version,
            )
        };

        if let Err(error) = result {
            return Err(error::SpannedError {
                error,
                span: Some((decimal_format.span.start..decimal_format.span.end).into()),
            });
        }
    }

    let named_decimal_formats = named_decimal_formats
        .into_iter()
        .map(|(name, format)| format.build().map(|format| (name, format)))
        .collect::<error::Result<HashMap<_, _>>>()?;
    static_context.set_decimal_formats(default_decimal_format.build()?, named_decimal_formats);
    Ok(())
}

fn compile_preprocessed_declarations(
    xslt: &str,
    declarations: Vec<PreprocessedDeclaration>,
    mut static_context: StaticContext,
    initial_mode: ast::ApplyTemplatesModeValue,
) -> error::SpannedResult<interpreter::Program> {
    let source_chunks = build_source_chunks(
        xslt,
        &declarations,
        static_context.static_base_uri().map(|uri| uri.to_string()),
    );
    augment_static_context_with_decimal_formats(&declarations, &mut static_context)?;
    let mut ir_converter = IrConverter::new(&static_context, initial_mode);
    let declarations = ir_converter.transform(&declarations)?;
    let mut program = compile_xslt(declarations, static_context)?;
    program.set_dynamic_xpath_evaluator(Box::new(XsltDynamicXPathEvaluator));
    program.set_transform_evaluator(Box::new(crate::transform::XsltTransformEvaluator));
    program.set_source(xslt.to_string());
    for (uri, start_offset, end_offset, source) in source_chunks {
        program.add_source_chunk(uri, start_offset, end_offset, source);
    }
    Ok(program)
}

fn build_source_chunks(
    xslt: &str,
    declarations: &[PreprocessedDeclaration],
    entry_stylesheet_uri: Option<String>,
) -> Vec<(String, usize, usize, Option<String>)> {
    let mut chunks = Vec::new();
    let mut seen_uris = std::collections::HashSet::new();
    let mut offset = 0usize;

    if let Some(uri) = entry_stylesheet_uri {
        let end_offset = xslt.len();
        chunks.push((uri.clone(), 0, end_offset, Some(xslt.to_string())));
        seen_uris.insert(uri);
        offset = end_offset;
    }

    for declaration in declarations {
        let Some(uri) = declaration.stylesheet_uri.as_ref() else {
            continue;
        };
        if !seen_uris.insert(uri.clone()) {
            continue;
        }
        let Some(source_len) = source_len_from_stylesheet_uri(uri) else {
            continue;
        };
        let start_offset = offset;
        let end_offset = start_offset + source_len;
        chunks.push((uri.clone(), start_offset, end_offset, None));
        offset = end_offset;
    }

    chunks
}

fn source_len_from_stylesheet_uri(uri: &str) -> Option<usize> {
    let path = uri.strip_prefix("file://")?.replace("%20", " ");
    std::fs::read_to_string(path)
        .ok()
        .map(|content| content.len())
}

pub fn parse(
    static_context: StaticContext,
    xslt: &str,
) -> error::SpannedResult<interpreter::Program> {
    parse_with_base_dir(static_context, xslt, std::env::current_dir().ok())
}

pub fn parse_with_base_dir(
    static_context: StaticContext,
    xslt: &str,
    base_dir: Option<std::path::PathBuf>,
) -> error::SpannedResult<interpreter::Program> {
    parse_with_base_dir_and_initial_mode(static_context, xslt, base_dir, None)
}

pub fn parse_with_base_dir_and_initial_mode(
    static_context: StaticContext,
    xslt: &str,
    base_dir: Option<std::path::PathBuf>,
    initial_mode: Option<String>,
) -> error::SpannedResult<interpreter::Program> {
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
    compile_preprocessed_declarations(xslt, declarations, static_context, initial_mode)
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

    for (prefix_id, namespace_id) in xot.namespaces_in_scope(document_element) {
        let prefix = xot.prefix_str(prefix_id);
        let namespace = xot.namespace_str(namespace_id);
        namespaces.add(&[(prefix, namespace)]);
    }
}

fn detect_stylesheet_version(xslt: &str) -> Option<u8> {
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

fn map_parse_error(xslt: &str, error: ElementError) -> error::SpannedError {
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
            },
            AttributeError::Unexpected { span, .. } => error::SpannedError {
                error: error::Error::XTSE0090,
                span: Some((span.start..span.end).into()),
            },
            AttributeError::Invalid { span, .. } | AttributeError::InvalidEqName { span, .. } => {
                error::SpannedError {
                    error: error::Error::XTSE0020,
                    span: Some((span.start..span.end).into()),
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
            },
            other => error::Error::Unsupported(format!("Failed parsing XSLT: {:?}", other)).into(),
        },
        ElementError::Unexpected { span } => {
            let text = xslt.get(span.start..span.end).unwrap_or_default();
            error::Error::Unsupported(format!(
                "Failed parsing XSLT, Unexpected {} {:?}",
                text, span
            ))
            .into()
        }
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

impl<'a> IrConverter<'a> {
    fn new(static_context: &'a StaticContext, initial_mode: ast::ApplyTemplatesModeValue) -> Self {
        IrConverter {
            variables: IrVariables::new(),
            static_context,
            overridden_static_context: None,
            initial_mode,
            xslt_functions: HashMap::new(),
            accumulator_declarations: HashMap::new(),
            referenced_accumulators: HashSet::new(),
            namespace_aliases: HashMap::new(),
            attribute_sets: HashMap::new(),
            named_outputs: HashMap::new(),
            character_maps: HashMap::new(),
            resolved_character_maps: HashMap::new(),
            active_attribute_sets: Vec::new(),
            secondary_result_document_depth: 0,
            named_templates_with_absent_context: HashSet::new(),
            template_continuation_available: false,
            strip_source_document_whitespace: false,
            number_patterns: Vec::new(),
        }
    }

    fn with_template_continuation_availability<T, F>(
        &mut self,
        available: bool,
        f: F,
    ) -> error::SpannedResult<T>
    where
        F: FnOnce(&mut Self) -> error::SpannedResult<T>,
    {
        let previous = self.template_continuation_available;
        self.template_continuation_available = available;
        let result = f(self);
        self.template_continuation_available = previous;
        result
    }

    fn with_attribute_set_variable_scope<T, F>(&mut self, f: F) -> error::SpannedResult<T>
    where
        F: FnOnce(&mut Self) -> error::SpannedResult<T>,
    {
        let local_scopes = self.variables.split_local_scopes();
        let result = f(self);
        self.variables.restore_local_scopes(local_scopes);
        result
    }

    fn current_static_context(&self) -> &StaticContext {
        self.overridden_static_context
            .as_ref()
            .unwrap_or(self.static_context)
    }

    fn with_static_base_uri<T, F>(
        &mut self,
        static_base_uri: Option<IriAbsoluteString>,
        f: F,
    ) -> error::SpannedResult<T>
    where
        F: FnOnce(&mut Self) -> error::SpannedResult<T>,
    {
        let previous = self.overridden_static_context.take();
        let base_context = previous.as_ref().unwrap_or(self.static_context);
        self.overridden_static_context =
            Some(base_context.clone_with_static_base_uri(static_base_uri));
        let result = f(self);
        self.overridden_static_context = previous;
        result
    }

    fn resolve_static_base_uri(&self, uri: &str) -> Option<IriAbsoluteString> {
        let reference: &IriReferenceStr = uri.try_into().ok()?;
        if let Some(base) = self.current_static_context().static_base_uri() {
            let resolved = reference.resolve_against(base);
            IriAbsoluteString::try_from(resolved.to_string()).ok()
        } else {
            IriAbsoluteString::try_from(uri.to_string()).ok()
        }
    }

    fn with_declaration_base_uri<T, F>(
        &mut self,
        declaration: &PreprocessedDeclaration,
        f: F,
    ) -> error::SpannedResult<T>
    where
        F: FnOnce(&mut Self) -> error::SpannedResult<T>,
    {
        if let Some(uri) = &declaration.stylesheet_uri {
            let iri = IriAbsoluteString::try_from(uri.clone()).ok();
            self.with_static_base_uri(iri, f)
        } else {
            f(self)
        }
    }

    fn raise_error(&mut self, error: RaisedError) -> Bindings {
        Bindings::empty().bind_expr_no_span(&mut self.variables, ir::Expr::RaiseError(error))
    }

    fn main_sequence_constructor(&mut self) -> ast::SequenceConstructor {
        vec![ast::SequenceConstructorItem::Instruction(
            ast::SequenceConstructorInstruction::ApplyTemplates(Box::new(ast::ApplyTemplates {
                mode: self.initial_mode.clone(),
                builtin_template_params_passthrough: true,
                select: ast::Expression {
                    xpath: xee_xpath_ast::ast::XPath::parse(
                        "/",
                        &Namespaces::default(),
                        &xee_name::VariableNames::new(),
                    )
                    .unwrap(),
                    span: xee_xslt_ast::ast::Span::new(0, 0),
                    namespaces: Vec::new(),
                },
                content: vec![],
                span: xee_xslt_ast::ast::Span::new(0, 0),
            })),
        )]
    }

    fn simple_content_atom(&mut self) -> ir::Atom {
        self.static_function_atom("simple-content", FN_NAMESPACE, 2)
    }

    fn concat_atom(&mut self, arity: u8) -> ir::Atom {
        self.static_function_atom("concat", FN_NAMESPACE, arity)
    }

    // fn error_atom(&mut self) -> ir::Atom {
    //     self.static_function_atom("error", Some(FN_NAMESPACE), 0)
    // }

    fn static_function_atom(&mut self, name: &str, namespace: &str, arity: u8) -> ir::Atom {
        ir::Atom::Const(ir::Const::StaticFunctionReference(
            self.current_static_context()
                .function_id_by_name(
                    &Name::new(name.to_string(), namespace.to_string(), String::new()),
                    arity,
                )
                .unwrap(),
            None,
        ))
    }

    fn static_function_call_expr(
        &mut self,
        name: &str,
        namespace: &str,
        arity: u8,
        args: Vec<ir::AtomS>,
    ) -> ir::Expr {
        ir::Expr::FunctionCall(ir::FunctionCall {
            atom: Spanned::new(
                self.static_function_atom(name, namespace, arity),
                (0..0).into(),
            ),
            args,
        })
    }

    fn simple_content_expr(
        &mut self,
        select_atom: ir::AtomS,
        separator_atom: ir::AtomS,
    ) -> ir::Expr {
        ir::Expr::FunctionCall(ir::FunctionCall {
            atom: Spanned::new(self.simple_content_atom(), (0..0).into()),
            args: vec![select_atom, separator_atom],
        })
    }

    fn processor_xslt_version(&self) -> u8 {
        self.current_static_context().processor_xslt_version().unwrap_or(3)
    }

    fn with_temporary_output_state(&mut self, body: Bindings) -> error::SpannedResult<Bindings> {
        let (body_atom, body_bindings) = self.zero_arg_closure(body);
        let expr = self.static_function_call_expr(
            "xslt-with-temporary-output-state",
            FN_NAMESPACE,
            1,
            vec![body_atom],
        );
        Ok(body_bindings.bind_expr_no_span(&mut self.variables, expr))
    }

    fn sequence_constructor_with_temporary_output_state(
        &mut self,
        sequence_constructor: &ast::SequenceConstructor,
    ) -> error::SpannedResult<Bindings> {
        let body = self.sequence_constructor(sequence_constructor)?;
        self.with_temporary_output_state(body)
    }

    fn select_or_sequence_constructor_with_temporary_output_state(
        &mut self,
        instruction: &impl ast::SelectOrSequenceConstructor,
    ) -> error::SpannedResult<Bindings> {
        if let Some(select) = instruction.select() {
            self.expression(select)
        } else {
            self.sequence_constructor_with_temporary_output_state(instruction.sequence_constructor())
        }
    }

    fn simple_content_bindings(
        &mut self,
        content_bindings: Bindings,
        separator_atom: ir::AtomS,
    ) -> Bindings {
        let (content_atom, content_bindings) = content_bindings.atom_bindings();
        let expr = self.simple_content_expr(content_atom, separator_atom);
        content_bindings.bind_expr_no_span(&mut self.variables, expr)
    }

    fn transform(
        &mut self,
        declarations: &[PreprocessedDeclaration],
    ) -> error::SpannedResult<ir::Declarations> {
        self.strip_source_document_whitespace =
            self.should_strip_source_document_whitespace(declarations);
        self.validate_reserved_declaration_names(declarations)?;
        self.register_xslt_function_names(declarations)?;
        self.collect_attribute_sets(declarations);
        self.validate_attribute_set_references()?;
        self.collect_named_outputs(declarations);
        self.collect_character_maps(declarations);
        self.collect_named_templates_with_absent_context(declarations);
        self.collect_namespace_aliases(declarations);
        self.collect_accumulators(declarations);
        // Register global variable/param names early so $var references resolve.
        let global_vars = self.collect_global_variables(declarations)?;

        let main_sequence_constructor = self.main_sequence_constructor();
        let main = self.sequence_constructor_function(&main_sequence_constructor)?;
        let mut ir_declarations = ir::Declarations::new(main);
        ir_declarations.global_variables = global_vars;

        for declaration in declarations {
            self.with_declaration_base_uri(declaration, |this| {
                this.declaration(&mut ir_declarations, declaration)
            })?;
        }

        self.compile_referenced_accumulators(&mut ir_declarations)?;

        // Move accumulated number patterns into IR declarations
        ir_declarations.number_patterns = std::mem::take(&mut self.number_patterns);

        Ok(ir_declarations)
    }

    fn should_strip_source_document_whitespace(
        &self,
        declarations: &[PreprocessedDeclaration],
    ) -> bool {
        let strip_all = declarations.iter().any(|declaration| {
            let ast::Declaration::StripSpace(strip_space) = &declaration.declaration else {
                return false;
            };

            strip_space.elements.iter().any(|element| element == "*")
        });

        strip_all
            && !declarations.iter().any(|declaration| {
                matches!(declaration.declaration, ast::Declaration::PreserveSpace(_))
            })
    }

    fn validate_reserved_declaration_names(
        &self,
        declarations: &[PreprocessedDeclaration],
    ) -> error::SpannedResult<()> {
        for declaration in declarations {
            match &declaration.declaration {
                ast::Declaration::Accumulator(accumulator) => {
                    validate_non_reserved_stylesheet_name(&accumulator.name, accumulator.span)?;
                }
                ast::Declaration::AttributeSet(attribute_set) => {
                    validate_non_reserved_stylesheet_name(&attribute_set.name, attribute_set.span)?;
                }
                ast::Declaration::CharacterMap(character_map) => {
                    validate_non_reserved_stylesheet_name(&character_map.name, character_map.span)?;
                }
                ast::Declaration::DecimalFormat(decimal_format) => {
                    if let Some(name) = &decimal_format.name {
                        validate_non_reserved_stylesheet_name(name, decimal_format.span)?;
                    }
                }
                ast::Declaration::Function(function) => {
                    validate_non_reserved_stylesheet_name(&function.name, function.span)?;
                }
                ast::Declaration::Key(key) => {
                    validate_non_reserved_stylesheet_name(&key.name, key.span)?;
                }
                ast::Declaration::Mode(mode) => {
                    if let Some(name) = &mode.name {
                        validate_non_reserved_stylesheet_name(name, mode.span)?;
                    }
                }
                ast::Declaration::Output(output) => {
                    if let Some(name) = &output.name {
                        validate_non_reserved_stylesheet_name(name, output.span)?;
                    }
                }
                ast::Declaration::Param(param) => {
                    validate_non_reserved_stylesheet_name(&param.name, param.span)?;
                }
                ast::Declaration::Template(template) => {
                    if let Some(name) = &template.name {
                        validate_non_reserved_stylesheet_name(name, template.span)?;
                    }
                }
                ast::Declaration::Variable(variable) => {
                    validate_non_reserved_stylesheet_name(&variable.name, variable.span)?;
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn collect_namespace_aliases(&mut self, declarations: &[PreprocessedDeclaration]) {
        for declaration in declarations {
            let ast::Declaration::NamespaceAlias(namespace_alias) = &declaration.declaration else {
                continue;
            };
            self.namespace_aliases.insert(
                namespace_alias.stylesheet_namespace.clone(),
                namespace_alias.result_namespace.clone(),
            );
        }
    }

    fn attribute_set_key(name: &ast::EqName) -> (String, String) {
        (name.namespace().to_string(), name.local_name().to_string())
    }

    fn character_map_key(name: &ast::EqName) -> (String, String) {
        (name.namespace().to_string(), name.local_name().to_string())
    }

    fn output_key(name: &ast::EqName) -> (String, String) {
        (name.namespace().to_string(), name.local_name().to_string())
    }

    fn collect_attribute_sets(&mut self, declarations: &[PreprocessedDeclaration]) {
        for declaration in declarations {
            let ast::Declaration::AttributeSet(attribute_set) = &declaration.declaration else {
                continue;
            };
            self.attribute_sets
                .entry(Self::attribute_set_key(&attribute_set.name))
                .or_default()
                .push((**attribute_set).clone());
        }
    }

    fn validate_attribute_set_references(&self) -> error::SpannedResult<()> {
        for key in self.attribute_sets.keys() {
            self.validate_attribute_set_references_for(key, &mut HashSet::new())?;
        }
        Ok(())
    }

    fn validate_attribute_set_references_for(
        &self,
        key: &(String, String),
        visiting: &mut HashSet<(String, String)>,
    ) -> error::SpannedResult<()> {
        if !visiting.insert(key.clone()) {
            return Ok(());
        }

        let Some(attribute_sets) = self.attribute_sets.get(key) else {
            return Err(error::Error::XTSE0710.into());
        };

        for attribute_set in attribute_sets {
            if let Some(use_attribute_sets) = &attribute_set.use_attribute_sets {
                for name in use_attribute_sets {
                    let nested_key = Self::attribute_set_key(name);
                    if !self.attribute_sets.contains_key(&nested_key) {
                        return Err(error::Error::XTSE0710.into());
                    }
                    self.validate_attribute_set_references_for(&nested_key, visiting)?;
                }
            }
        }

        visiting.remove(key);
        Ok(())
    }

    fn collect_character_maps(&mut self, declarations: &[PreprocessedDeclaration]) {
        for declaration in declarations {
            let ast::Declaration::CharacterMap(character_map) = &declaration.declaration else {
                continue;
            };
            let key = Self::character_map_key(&character_map.name);
            let should_replace = match self.character_maps.get(&key) {
                Some((precedence, _)) => declaration.import_precedence >= *precedence,
                None => true,
            };
            if should_replace {
                self.character_maps.insert(
                    key,
                    (declaration.import_precedence, (**character_map).clone()),
                );
            }
        }
    }

    fn collect_named_outputs(&mut self, declarations: &[PreprocessedDeclaration]) {
        for declaration in declarations {
            let ast::Declaration::Output(output) = &declaration.declaration else {
                continue;
            };
            let Some(name) = &output.name else {
                continue;
            };
            let key = Self::output_key(name);
            let should_replace = match self.named_outputs.get(&key) {
                Some((precedence, _)) => declaration.import_precedence >= *precedence,
                None => true,
            };
            if should_replace {
                self.named_outputs
                    .insert(key, (declaration.import_precedence, (**output).clone()));
            }
        }
    }

    fn resolve_character_maps(
        &mut self,
        names: &[ast::EqName],
    ) -> error::SpannedResult<ahash::HashMap<char, String>> {
        let mut resolved = ahash::HashMap::default();
        let mut visiting = HashSet::new();
        for name in names {
            for (character, replacement) in self.resolve_character_map(name, &mut visiting)? {
                resolved.insert(character, replacement);
            }
        }
        Ok(resolved)
    }

    fn resolve_character_map(
        &mut self,
        name: &ast::EqName,
        visiting: &mut HashSet<(String, String)>,
    ) -> error::SpannedResult<ahash::HashMap<char, String>> {
        let key = Self::character_map_key(name);
        if let Some(resolved) = self.resolved_character_maps.get(&key) {
            return Ok(resolved.clone());
        }
        if !visiting.insert(key.clone()) {
            return Err(error::Error::Unsupported(
                "Circular xsl:character-map dependencies are not supported yet".to_string(),
            )
            .into());
        }

        let Some((_, character_map)) = self.character_maps.get(&key).cloned() else {
            return Err(error::Error::Unsupported(format!(
                "Unknown xsl:character-map {}",
                name.local_name()
            ))
            .into());
        };

        let mut resolved = ahash::HashMap::default();
        if let Some(used_character_maps) = &character_map.use_character_maps {
            for used_character_map in used_character_maps {
                for (character, replacement) in
                    self.resolve_character_map(used_character_map, visiting)?
                {
                    resolved.insert(character, replacement);
                }
            }
        }
        for output_character in character_map.output_characters {
            resolved.insert(output_character.character, output_character.string);
        }

        visiting.remove(&key);
        self.resolved_character_maps
            .insert(key.clone(), resolved.clone());
        Ok(resolved)
    }

    fn encode_character_maps(character_maps: &ahash::HashMap<char, String>) -> String {
        let mut entries = character_maps.iter().collect::<Vec<_>>();
        entries.sort_by_key(|(character, _)| **character as u32);
        entries
            .into_iter()
            .map(|(character, replacement)| {
                format!(
                    "{:x}={}",
                    *character as u32,
                    replacement
                        .as_bytes()
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>()
                )
            })
            .collect::<Vec<_>>()
            .join("|")
    }

    fn encode_named_outputs(&mut self) -> error::SpannedResult<String> {
        let mut outputs = self
            .named_outputs
            .iter()
            .map(|((namespace, local_name), (_, output))| {
                (namespace.clone(), local_name.clone(), output.clone())
            })
            .collect::<Vec<_>>();
        outputs.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));

        outputs
            .into_iter()
            .map(|(namespace, local_name, output)| {
                let fields = vec![
                    namespace,
                    local_name,
                    Self::output_method_literal(output.method.as_ref())?.unwrap_or_default(),
                    output.byte_order_mark.to_string(),
                    Self::qname_list_literal(&output.cdata_section_elements),
                    output.doctype_public.unwrap_or_default(),
                    output.doctype_system.unwrap_or_default(),
                    output.include_content_type.to_string(),
                    output.media_type.unwrap_or_default(),
                    output.item_separator.unwrap_or_default(),
                    output.omit_xml_declaration.to_string(),
                    Self::output_standalone_literal(output.standalone.as_ref()).unwrap_or_default(),
                    output
                        .html_version
                        .map(|html_version| html_version.to_string())
                        .unwrap_or_default(),
                    Self::encode_character_maps(&self.resolve_character_maps(&output.use_character_maps)?),
                    output.version.unwrap_or_default(),
                ];
                Ok(fields
                    .into_iter()
                    .map(|field| Self::hex_encode(&field))
                    .collect::<Vec<_>>()
                    .join(","))
            })
            .collect::<error::SpannedResult<Vec<_>>>()
            .map(|entries| entries.join(";"))
    }

    fn resolve_named_output(
        &self,
        format: &ast::ValueTemplate<ast::EqName>,
        namespaces: &[ast::LiteralNamespace],
    ) -> error::SpannedResult<ast::Output> {
        let lexical_qname = self.static_value_template(format).ok_or_else(|| {
            error::Error::Unsupported(
                "Dynamic xsl:result-document @format is not supported yet".to_string(),
            )
        })?;
        let key = if let Some((local_name, namespace)) =
            self.resolve_static_qname_with_default(&lexical_qname, namespaces, "")
        {
            (namespace, local_name)
        } else {
            (String::new(), lexical_qname)
        };
        let Some((_, output)) = self.named_outputs.get(&key) else {
            return Err(error::Error::Unsupported(
                "Unknown xsl:result-document @format".to_string(),
            )
            .into());
        };
        Ok(output.clone())
    }

    fn output_method_literal(
        method: Option<&ast::OutputMethod>,
    ) -> error::SpannedResult<Option<String>> {
        Ok(match method {
            Some(ast::OutputMethod::Adaptive) => Some("adaptive".to_string()),
            Some(ast::OutputMethod::Xml) => Some("xml".to_string()),
            Some(ast::OutputMethod::Html) => Some("html".to_string()),
            Some(ast::OutputMethod::Xhtml) => Some("xhtml".to_string()),
            Some(ast::OutputMethod::Text) => Some("text".to_string()),
            Some(ast::OutputMethod::Json) => Some("json".to_string()),
            None => None,
            method => {
                return Err(error::Error::Unsupported(format!(
                    "Output method {:?} not supported yet",
                    method
                ))
                .into())
            }
        })
    }

    fn output_standalone_literal(standalone: Option<&ast::Standalone>) -> Option<String> {
        match standalone {
            Some(ast::Standalone::Bool(true)) => Some("yes".to_string()),
            Some(ast::Standalone::Bool(false)) => Some("no".to_string()),
            Some(ast::Standalone::Omit) => Some("omit".to_string()),
            None => None,
        }
    }

    fn validate_standalone_literal(value: &str) -> error::SpannedResult<String> {
        let trimmed = value.trim();
        match trimmed {
            "yes" | "true" | "1" | "no" | "false" | "0" | "omit" => Ok(trimmed.to_string()),
            _ => Err(error::Error::XTSE0020.into()),
        }
    }

    fn validate_boolean_literal(value: &str) -> error::SpannedResult<String> {
        let trimmed = value.trim();
        match trimmed {
            "yes" | "true" | "1" | "no" | "false" | "0" => Ok(trimmed.to_string()),
            _ => Err(error::Error::XTSE0020.into()),
        }
    }

    fn value_template_or_literal_atom<V>(
        &mut self,
        value_template: Option<&ast::ValueTemplate<V>>,
        default: String,
    ) -> error::SpannedResult<(ir::AtomS, Bindings)>
    where
        V: Clone + PartialEq + Eq,
    {
        Ok(if let Some(value_template) = value_template {
            if let Some(literal) = self.static_value_template(value_template) {
                (
                    Spanned::new(ir::Atom::Const(ir::Const::String(literal)), (0..0).into()),
                    Bindings::empty(),
                )
            } else {
                self.attribute_value_template(value_template)?.atom_bindings()
            }
        } else {
            (
                Spanned::new(ir::Atom::Const(ir::Const::String(default)), (0..0).into()),
                Bindings::empty(),
            )
        })
    }

    fn validated_value_template_or_literal_atom<V>(
        &mut self,
        value_template: Option<&ast::ValueTemplate<V>>,
        default: String,
        validator: fn(&str) -> error::SpannedResult<String>,
    ) -> error::SpannedResult<(ir::AtomS, Bindings)>
    where
        V: Clone + PartialEq + Eq,
    {
        Ok(if let Some(value_template) = value_template {
            if let Some(literal) = self.static_value_template(value_template) {
                (
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String(validator(&literal)?)),
                        (0..0).into(),
                    ),
                    Bindings::empty(),
                )
            } else {
                self.attribute_value_template(value_template)?.atom_bindings()
            }
        } else {
            (
                Spanned::new(ir::Atom::Const(ir::Const::String(default)), (0..0).into()),
                Bindings::empty(),
            )
        })
    }

    fn qname_list_literal(names: &[ast::EqName]) -> String {
        names
            .iter()
            .map(|name| {
                if name.prefix().is_empty() {
                    name.local_name().to_string()
                } else {
                    format!("{}:{}", name.prefix(), name.local_name())
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn merge_literal_qname_lists(base: &[ast::EqName], extra: Option<String>) -> String {
        let mut merged = base
            .iter()
            .map(|name| {
                if name.prefix().is_empty() {
                    name.local_name().to_string()
                } else {
                    format!("{}:{}", name.prefix(), name.local_name())
                }
            })
            .collect::<Vec<_>>();
        if let Some(extra) = extra {
            for token in extra.split_ascii_whitespace() {
                if !merged.iter().any(|existing| existing == token) {
                    merged.push(token.to_string());
                }
            }
        }
        merged.join(" ")
    }

    fn collect_named_templates_with_absent_context(
        &mut self,
        declarations: &[PreprocessedDeclaration],
    ) {
        for declaration in declarations {
            let ast::Declaration::Template(template) = &declaration.declaration else {
                continue;
            };
            let Some(name) = &template.name else {
                continue;
            };
            let has_absent_context = matches!(
                template
                    .context_item
                    .as_ref()
                    .and_then(|context_item| context_item.use_.as_ref()),
                Some(ast::Use::Absent)
            );
            if has_absent_context {
                self.named_templates_with_absent_context
                    .insert(name.local_name().to_string());
            }
        }
    }

    fn collect_accumulators(&mut self, declarations: &[PreprocessedDeclaration]) {
        for declaration in declarations {
            let ast::Declaration::Accumulator(accumulator) = &declaration.declaration else {
                continue;
            };

            let should_replace = self
                .accumulator_declarations
                .get(&accumulator.name)
                .map(|existing| declaration.import_precedence >= existing.import_precedence)
                .unwrap_or(true);
            if should_replace {
                self.accumulator_declarations
                    .insert(accumulator.name.clone(), declaration.clone());
            }
        }
    }

    fn apply_namespace_alias(&self, name: &ast::Name) -> ast::Name {
        let Some(namespace) = self.namespace_aliases.get(name.namespace()) else {
            return name.clone();
        };

        let prefix = if namespace.is_empty() {
            String::new()
        } else {
            name.prefix().to_string()
        };
        Name::new(name.local_name().to_string(), namespace.clone(), prefix)
    }

    fn register_xslt_function_names(
        &mut self,
        declarations: &[PreprocessedDeclaration],
    ) -> error::SpannedResult<()> {
        for declaration in declarations {
            let ast::Declaration::Function(function) = &declaration.declaration else {
                continue;
            };

            let arity = u8::try_from(function.params.len()).map_err(|_| {
                error::Error::Unsupported("Too many XSLT function parameters".to_string())
            })?;

            let hidden_name = OwnedName::new(
                format!("function-{}", self.xslt_functions.len()),
                "urn:xee:internal:function".to_string(),
                "xee-internal".to_string(),
            );
            self.xslt_functions
                .insert((function.name.clone(), arity), hidden_name.clone());
            self.variables.new_var_name(&hidden_name);
        }

        Ok(())
    }

    fn declaration(
        &mut self,
        declarations: &mut ir::Declarations,
        declaration: &PreprocessedDeclaration,
    ) -> error::SpannedResult<()> {
        use ast::Declaration::*;
        match &declaration.declaration {
            AttributeSet(_) => Ok(()),
            Template(template) => self.template(
                declarations,
                template,
                declaration.import_precedence,
                &declaration.module_path,
            ),
            Mode(mode) => self.mode(declarations, mode),
            Output(output) => self.output(declarations, output),
            // Import/Include already handled during pre-processing in parse_with_base_dir
            Import(_) | Include(_) => Ok(()),
            Key(key) => self.key(declarations, key),
            // These declarations are parsed but not yet compiled - skip gracefully
            // to allow stylesheets containing them to still process templates
            Function(_) | Variable(_) | Param(_) | StripSpace(_) | PreserveSpace(_)
            | DecimalFormat(_) | CharacterMap(_) | NamespaceAlias(_) | ImportSchema(_)
            | UsePackage(_) | GlobalContextItem(_) | Accumulator(_) => Ok(()),
        }
    }

    fn compile_referenced_accumulators(
        &mut self,
        declarations: &mut ir::Declarations,
    ) -> error::SpannedResult<()> {
        let mut compiled = HashSet::new();

        loop {
            let pending = self
                .referenced_accumulators
                .iter()
                .filter(|name| !compiled.contains(*name))
                .cloned()
                .collect::<Vec<_>>();
            if pending.is_empty() {
                break;
            }

            for name in pending {
                compiled.insert(name.clone());

                let Some(declaration) = self.accumulator_declarations.get(&name).cloned() else {
                    continue;
                };
                let ast::Declaration::Accumulator(accumulator) = &declaration.declaration else {
                    continue;
                };

                let accumulator_definition = self.with_declaration_base_uri(&declaration, |this| {
                    this.accumulator(accumulator)
                })?;
                declarations.accumulators.push(accumulator_definition);
            }
        }

        Ok(())
    }

    fn collect_global_variables(
        &mut self,
        declarations: &[PreprocessedDeclaration],
    ) -> error::SpannedResult<Vec<ir::GlobalVariable>> {
        for decl in declarations {
            match &decl.declaration {
                ast::Declaration::Variable(var) => {
                    self.variables.new_var_name(&var.name);
                }
                ast::Declaration::Param(param) => {
                    self.variables.new_var_name(&param.name);
                }
                ast::Declaration::Function(function) => {
                    let arity = u8::try_from(function.params.len()).map_err(|_| {
                        error::Error::Unsupported("Too many XSLT function parameters".to_string())
                    })?;
                    let hidden_name = self
                        .xslt_functions
                        .get(&(function.name.clone(), arity))
                        .ok_or_else(|| {
                            error::Error::Unsupported("Unregistered XSLT function name".to_string())
                        })?;
                    self.variables.new_var_name(hidden_name);
                }
                _ => {}
            }
        }

        let mut globals = Vec::new();
        for decl in declarations {
            match &decl.declaration {
                ast::Declaration::Variable(var) => {
                    self.validate_variable(var)?;
                    let name = self.variables.lookup_var_name(&var.name).unwrap();
                    let expr = self.with_declaration_base_uri(decl, |this| {
                        this.with_hidden_global_name(&var.name, |this| {
                            let context_names = this.variables.push_context();
                            let params = Self::context_params(&context_names);
                            let expr = this.global_variable_expr(
                                var.select.as_ref(),
                                &var.sequence_constructor,
                                var.as_.as_ref(),
                            )?;
                            this.variables.pop_context();
                            Ok((params, expr))
                        })
                    })?;
                    globals.push(ir::GlobalVariable {
                        name,
                        original_name: Some(var.name.clone()),
                        public_name: None,
                        public_arity: None,
                        external: false,
                        required: false,
                        params: expr.0,
                        expr: expr.1,
                    });
                }
                ast::Declaration::Param(param) => {
                    self.validate_param(param)?;
                    let name = self.variables.lookup_var_name(&param.name).unwrap();
                    let expr = self.with_declaration_base_uri(decl, |this| {
                        this.with_hidden_global_name(&param.name, |this| {
                            let context_names = this.variables.push_context();
                            let params = Self::context_params(&context_names);
                            let expr = this.global_param_expr(
                                param.select.as_ref(),
                                &param.sequence_constructor,
                            )?;
                            this.variables.pop_context();
                            Ok((params, expr))
                        })
                    })?;
                    globals.push(ir::GlobalVariable {
                        name,
                        original_name: Some(param.name.clone()),
                        public_name: None,
                        public_arity: None,
                        external: true,
                        required: param.required,
                        params: expr.0,
                        expr: expr.1,
                    });
                }
                ast::Declaration::Function(function) => {
                    let arity = u8::try_from(function.params.len()).map_err(|_| {
                        error::Error::Unsupported("Too many XSLT function parameters".to_string())
                    })?;
                    let hidden_name = self
                        .xslt_functions
                        .get(&(function.name.clone(), arity))
                        .ok_or_else(|| {
                            error::Error::Unsupported("Unregistered XSLT function name".to_string())
                        })?;
                    let name = self.variables.lookup_var_name(hidden_name).unwrap();
                    let (params, expr) = self.with_declaration_base_uri(decl, |this| {
                        let context_names = this.variables.push_context();
                        let params = Self::context_params(&context_names);
                        let function_definition = this.xslt_function_definition(function)?;
                        this.variables.pop_context();
                        let expr = Spanned::new(
                            ir::Expr::FunctionDefinition(function_definition),
                            (function.span.start..function.span.end).into(),
                        );
                        Ok((params, expr))
                    })?;
                    let (public_name, public_arity) = match function.visibility {
                        Some(ast::VisibilityWithAbstract::Public)
                        | Some(ast::VisibilityWithAbstract::Final) => {
                            (Some(function.name.clone()), Some(arity))
                        }
                        _ => (None, None),
                    };
                    globals.push(ir::GlobalVariable {
                        name,
                        original_name: None,
                        public_name,
                        public_arity,
                        external: false,
                        required: false,
                        params,
                        expr,
                    });
                }
                _ => {}
            }
        }
        Ok(globals)
    }

    fn with_hidden_global_name<T, F>(&mut self, name: &ast::Name, f: F) -> error::SpannedResult<T>
    where
        F: FnOnce(&mut Self) -> error::SpannedResult<T>,
    {
        let hidden_name = self.variables.remove_var_name_in_current_scope(name);
        let result = f(self);
        if let Some(hidden_name) = hidden_name {
            self.variables
                .insert_var_name_in_current_scope(name.clone(), hidden_name);
        }
        result
    }

    fn validate_param(&self, param: &ast::Param) -> error::SpannedResult<()> {
        if param.required && (param.select.is_some() || !param.sequence_constructor.is_empty()) {
            return Err(error::Error::XTSE0010.into());
        }
        if param.select.is_some()
            && Self::has_non_empty_binding_content(&param.sequence_constructor)
        {
            return Err(error::Error::XTSE0620.into());
        }
        Ok(())
    }

    fn validate_variable(&self, variable: &ast::Variable) -> error::SpannedResult<()> {
        if variable.select.is_some()
            && Self::has_non_empty_binding_content(&variable.sequence_constructor)
        {
            return Err(error::Error::XTSE0620.into());
        }
        Ok(())
    }

    fn has_non_empty_binding_content(sequence_constructor: &ast::SequenceConstructor) -> bool {
        sequence_constructor.iter().any(|item| match item {
            ast::SequenceConstructorItem::Content(ast::Content::Text(text)) => {
                !text.trim().is_empty()
            }
            _ => true,
        })
    }

    fn global_variable_expr(
        &mut self,
        select: Option<&ast::Expression>,
        sequence_constructor: &ast::SequenceConstructor,
        sequence_type: Option<&xpath_ast::SequenceType>,
    ) -> error::SpannedResult<ir::ExprS> {
        let expr = if let Some(select) = select {
            self.expression(select)?.expr()
        } else if sequence_type.is_some() {
            self.sequence_constructor(sequence_constructor)?.expr()
        } else if !sequence_constructor.is_empty() {
            self.temporary_tree(sequence_constructor)?.expr()
        } else {
            Spanned::new(ir::Expr::Atom(self.empty_string()), (0..0).into())
        };
        self.convert_expr(expr, sequence_type, RaisedError::XTTE0570)
    }

    fn global_param_expr(
        &mut self,
        select: Option<&ast::Expression>,
        sequence_constructor: &ast::SequenceConstructor,
    ) -> error::SpannedResult<ir::ExprS> {
        let expr = if let Some(select) = select {
            self.expression(select)?.expr()
        } else if !sequence_constructor.is_empty() {
            self.sequence_constructor(sequence_constructor)?.expr()
        } else {
            self.empty_sequence()
        };
        Ok(expr)
    }

    fn convert_expr(
        &mut self,
        expr: ir::ExprS,
        sequence_type: Option<&xpath_ast::SequenceType>,
        error: RaisedError,
    ) -> error::SpannedResult<ir::ExprS> {
        let Some(sequence_type) = sequence_type else {
            return Ok(expr);
        };

        let binding = self.variables.new_binding(expr.value, expr.span);
        let (atom, bindings) = Bindings::new(binding).atom_bindings();
        Ok(bindings
            .bind_expr_no_span(
                &mut self.variables,
                ir::Expr::ConvertSequence(ir::ConvertSequence {
                    atom,
                    sequence_type: sequence_type.clone(),
                    error,
                }),
            )
            .expr())
    }

    fn convert_bindings(
        &mut self,
        bindings: Bindings,
        sequence_type: Option<&xpath_ast::SequenceType>,
        error: RaisedError,
    ) -> error::SpannedResult<Bindings> {
        let Some(sequence_type) = sequence_type else {
            return Ok(bindings);
        };

        let (atom, bindings) = bindings.atom_bindings();
        Ok(bindings.bind_expr_no_span(
            &mut self.variables,
            ir::Expr::ConvertSequence(ir::ConvertSequence {
                atom,
                sequence_type: sequence_type.clone(),
                error,
            }),
        ))
    }

    fn with_param(
        &mut self,
        with_param: &ast::WithParam,
    ) -> error::SpannedResult<(ir::WithParam, Bindings)> {
        self.with_param_with_error(with_param, RaisedError::XTTE0570)
    }

    fn evaluate_with_param(
        &mut self,
        with_param: &ast::WithParam,
    ) -> error::SpannedResult<(ir::WithParam, Bindings)> {
        self.with_param_with_error(with_param, RaisedError::XTTE0590)
    }

    fn with_param_with_error(
        &mut self,
        with_param: &ast::WithParam,
        error: RaisedError,
    ) -> error::SpannedResult<(ir::WithParam, Bindings)> {
        if with_param.select.is_some()
            && Self::has_non_empty_binding_content(&with_param.sequence_constructor)
        {
            return Err(error::Error::XTSE0620.into());
        }
        let bindings = if let Some(select) = &with_param.select {
            self.expression(select)?
        } else if with_param.sequence_constructor.is_empty() {
            let expr = self.empty_sequence();
            Bindings::new(self.variables.new_binding(expr.value, expr.span))
        } else {
            self.sequence_constructor_with_temporary_output_state(&with_param.sequence_constructor)?
        };

        let bindings = self.convert_bindings(bindings, with_param.as_.as_ref(), error)?;
        let (select_atom, bindings) = bindings.atom_bindings();

        Ok((
            ir::WithParam {
                name: ir::Name::new(with_param.name.local_name().to_string()),
                select: Some(select_atom),
                sequence_constructor: None,
                tunnel: with_param.tunnel,
            },
            bindings,
        ))
    }

    fn context_params(context_names: &ir::ContextNames) -> Vec<ir::Param> {
        vec![
            ir::Param {
                name: context_names.item.clone(),
                type_: None,
                default: None,
                required: false,
                original_name: None,
                tunnel: false,
            },
            ir::Param {
                name: context_names.position.clone(),
                type_: None,
                default: None,
                required: false,
                original_name: None,
                tunnel: false,
            },
            ir::Param {
                name: context_names.last.clone(),
                type_: None,
                default: None,
                required: false,
                original_name: None,
                tunnel: false,
            },
        ]
    }

    fn key(
        &mut self,
        declarations: &mut ir::Declarations,
        key: &ast::Key,
    ) -> error::SpannedResult<()> {
        // Compile the use expression (or sequence constructor) into a function
        // that takes a context node and returns the key value(s).
        let context_names = self.variables.push_context();
        let bindings = if let Some(use_expr) = &key.use_ {
            self.expression(use_expr)?
        } else if !key.sequence_constructor.is_empty() {
            self.sequence_constructor_with_temporary_output_state(&key.sequence_constructor)?
        } else {
            self.variables.pop_context();
            return Err(error::Error::Unsupported(
                "xsl:key without use= attribute or body is not yet supported".to_string(),
            )
            .into());
        };
        self.variables.pop_context();
        let use_function = ir::FunctionDefinition {
            params: Self::context_params(&context_names),
            return_type: None,
            body: Box::new(bindings.expr()),
        };

        // Compile the match pattern
        let pattern = transform_pattern(&key.match_.pattern, |expr| self.pattern_predicate(expr))?;

        let name = key.name.clone();
        declarations.keys.push(ir::KeyDefinition {
            name,
            pattern,
            use_function,
        });

        Ok(())
    }

    fn accumulator(
        &mut self,
        accumulator: &ast::Accumulator,
    ) -> error::SpannedResult<ir::AccumulatorDefinition> {
        let mut rules = Vec::with_capacity(accumulator.rules.len());
        for rule in &accumulator.rules {
            rules.push(self.accumulator_rule(rule)?);
        }

        Ok(ir::AccumulatorDefinition {
            name: accumulator.name.clone(),
            rules,
        })
    }

    fn accumulator_rule(
        &mut self,
        rule: &ast::AccumulatorRule,
    ) -> error::SpannedResult<ir::AccumulatorRuleDefinition> {
        let context_names = self.variables.push_context();
        self.variables.push_scope();

        let value_original_name = OwnedName::new("value".to_string(), String::new(), String::new());
        let value_name = self.variables.declare_var_name(&value_original_name);

        let bindings = if let Some(select) = &rule.select {
            self.expression(select)?
        } else if !rule.sequence_constructor.is_empty() {
            self.sequence_constructor(&rule.sequence_constructor)?
        } else {
            Bindings::empty()
        };

        self.variables.pop_scope();
        self.variables.pop_context();

        let mut params = Self::context_params(&context_names);
        params.push(ir::Param {
            name: value_name,
            type_: None,
            default: None,
            required: false,
            original_name: Some("value".to_string()),
            tunnel: false,
        });

        let pattern = transform_pattern(&rule.match_.pattern, |expr| self.pattern_predicate(expr))?;
        let phase = match rule.phase {
            Some(ast::AccumulatorPhase::End) => ir::AccumulatorPhase::End,
            Some(ast::AccumulatorPhase::Start) | None => ir::AccumulatorPhase::Start,
        };

        Ok(ir::AccumulatorRuleDefinition {
            pattern,
            phase,
            probe_temporary_output_state: !rule.sequence_constructor.is_empty(),
            rule_function: ir::FunctionDefinition {
                params,
                return_type: None,
                body: Box::new(bindings.expr()),
            },
        })
    }

    fn template(
        &mut self,
        declarations: &mut ir::Declarations,
        template: &ast::Template,
        import_precedence: i64,
        module_path: &[usize],
    ) -> error::SpannedResult<()> {
        for param in &template.params {
            self.validate_param(param)?;
        }
        let named_template_function = if let Some(name) = &template.name {
            Some(ir::FunctionBinding {
                name: ir::Name::new(name.local_name().to_string()),
                main: self.template_with_params_function(template)?,
            })
        } else {
            None
        };

        if let Some(pattern) = &template.match_ {
            let function_definition = self.matched_template_function(template)?;
            let modes = template
                .mode
                .iter()
                .map(Self::ast_mode_value_to_ir_mode_value)
                .collect::<Vec<_>>();

            if let Some(priority) = &template.priority {
                declarations.rules.push(ir::Rule {
                    import_precedence,
                    module_path: module_path.to_vec(),
                    priority: *priority,
                    modes,
                    pattern: transform_pattern(&pattern.pattern, |expr| {
                        self.pattern_predicate(expr)
                    })?,
                    function_definition,
                });
                if let Some(function_binding) = named_template_function {
                    declarations.functions.push(function_binding);
                }
                return Ok(());
            }

            let default_priorities = default_priority(&pattern.pattern).collect::<Vec<_>>();
            for (split_pattern, priority) in default_priorities {
                declarations.rules.push(ir::Rule {
                    import_precedence,
                    module_path: module_path.to_vec(),
                    priority,
                    modes: modes.clone(),
                    pattern: transform_pattern(&split_pattern, |expr| {
                        self.pattern_predicate(expr)
                    })?,
                    function_definition: function_definition.clone(),
                });
            }
            if let Some(function_binding) = named_template_function {
                declarations.functions.push(function_binding);
            }
            Ok(())
        } else if let Some(function_binding) = named_template_function {
            declarations.functions.push(function_binding);
            Ok(())
        } else {
            Err(error::Error::Unsupported(
                "Template must have either match or name attribute".to_string(),
            )
            .into())
        }
    }

    fn template_with_params_function(
        &mut self,
        template: &ast::Template,
    ) -> error::SpannedResult<ir::FunctionDefinition> {
        self.with_template_continuation_availability(true, |this| {
            let context_names = this.template_context_names(template);
            this.variables.push_scope();
            let param_names = this.register_template_param_names(template)?;

            let bindings = this.sequence_constructor(&template.sequence_constructor)?;
            let mut params = Self::context_params(&context_names);
            params.extend(this.template_params(template, param_names)?);
            this.variables.pop_scope();
            this.variables.pop_context();

            Ok(ir::FunctionDefinition {
                params,
                return_type: None,
                body: Box::new(bindings.expr()),
            })
        })
    }

    fn matched_template_function(
        &mut self,
        template: &ast::Template,
    ) -> error::SpannedResult<ir::FunctionDefinition> {
        self.with_template_continuation_availability(true, |this| {
            let context_names = this.template_context_names(template);
            this.variables.push_scope();
            let param_names = this.register_template_param_names(template)?;
            let bindings = this.sequence_constructor(&template.sequence_constructor)?;

            let mut params = Self::context_params(&context_names);
            params.extend(this.template_params(template, param_names)?);
            this.variables.pop_scope();
            this.variables.pop_context();

            Ok(ir::FunctionDefinition {
                params,
                return_type: None,
                body: Box::new(bindings.expr()),
            })
        })
    }

    fn template_context_names(&mut self, template: &ast::Template) -> ir::ContextNames {
        let has_absent_context = matches!(
            template
                .context_item
                .as_ref()
                .and_then(|context_item| context_item.use_.as_ref()),
            Some(ast::Use::Absent)
        );
        if has_absent_context {
            let item_name = self.variables.new_name();
            let context_names = self.variables.explicit_context_names(item_name);
            self.variables.push_absent_context();
            context_names
        } else {
            self.variables.push_context()
        }
    }

    fn xslt_function_definition(
        &mut self,
        function: &ast::Function,
    ) -> error::SpannedResult<ir::FunctionDefinition> {
        self.variables.push_absent_context();
        self.variables.push_scope();

        let mut params = Vec::new();
        let mut seen_names = HashSet::new();
        for param in &function.params {
            let param_key = (
                param.name.namespace().to_string(),
                param.name.local_name().to_string(),
            );
            if !seen_names.insert(param_key) {
                return Err(error::Error::Unsupported(
                    "Duplicate XSLT function parameters are not supported".to_string(),
                )
                .into());
            }

            let name = self.variables.declare_var_name(&param.name);
            params.push(ir::Param {
                name,
                type_: param.as_.clone(),
                default: None,
                required: true,
                original_name: None,
                tunnel: false,
            });
        }

        let bindings = self.sequence_constructor_with_temporary_output_state(
            &function.sequence_constructor,
        )?;

        self.variables.pop_scope();
        self.variables.pop_context();

        Ok(ir::FunctionDefinition {
            params,
            return_type: function.as_.clone(),
            body: Box::new(bindings.expr()),
        })
    }

    fn register_template_param_names(
        &mut self,
        template: &ast::Template,
    ) -> error::SpannedResult<Vec<(String, ir::Name)>> {
        let mut param_names = Vec::new();
        let mut seen_names = HashSet::new();
        for param in &template.params {
            let param_key = (
                param.name.namespace().to_string(),
                param.name.local_name().to_string(),
            );
            if !seen_names.insert(param_key) {
                return Err(error::SpannedError {
                    error: error::Error::XTSE0580,
                    span: Some((param.span.start..param.span.end).into()),
                });
            }
            let var_name = self.variables.declare_var_name(&param.name);
            param_names.push((param.name.local_name().to_string(), var_name));
        }
        Ok(param_names)
    }

    fn template_params(
        &mut self,
        template: &ast::Template,
        param_names: Vec<(String, ir::Name)>,
    ) -> error::SpannedResult<Vec<ir::Param>> {
        let mut params = Vec::new();
        for (original_name, runtime_name) in param_names {
            let ast_param = template
                .params
                .iter()
                .find(|param| param.name.local_name() == original_name);
            let required = ast_param.map(|param| param.required).unwrap_or(false);
            let param_type = ast_param.and_then(|param| param.as_.clone());

            let default = if let Some(ast_param) = ast_param {
                if !ast_param.sequence_constructor.is_empty() {
                    let expr_s = self
                        .sequence_constructor_with_temporary_output_state(
                            &ast_param.sequence_constructor,
                        )?
                        .expr();
                    let expr_s =
                        self.convert_expr(expr_s, ast_param.as_.as_ref(), RaisedError::XTTE0590)?;
                    Some(Box::new(expr_s.value))
                } else if let Some(select_expr) = &ast_param.select {
                    let expr_s = self.expression(select_expr)?.expr();
                    let expr_s =
                        self.convert_expr(expr_s, ast_param.as_.as_ref(), RaisedError::XTTE0590)?;
                    Some(Box::new(expr_s.value))
                } else {
                    None
                }
            } else {
                None
            };

            params.push(ir::Param {
                name: runtime_name,
                type_: param_type,
                default,
                required,
                original_name: Some(original_name),
                tunnel: ast_param.map(|param| param.tunnel).unwrap_or(false),
            });
        }
        Ok(params)
    }

    fn mode(
        &mut self,
        declarations: &mut ir::Declarations,
        mode: &ast::Mode,
    ) -> error::SpannedResult<()> {
        declarations.modes.insert(
            mode.name.clone(),
            ir::Mode {
                on_no_match: match mode.on_no_match.as_ref() {
                    Some(ast::OnNoMatch::DeepCopy) => ir::ModeOnNoMatch::DeepCopy,
                    Some(ast::OnNoMatch::ShallowCopy) => ir::ModeOnNoMatch::ShallowCopy,
                    Some(ast::OnNoMatch::DeepSkip) => ir::ModeOnNoMatch::DeepSkip,
                    Some(ast::OnNoMatch::ShallowSkip) => ir::ModeOnNoMatch::ShallowSkip,
                    Some(ast::OnNoMatch::Fail) => ir::ModeOnNoMatch::Fail,
                    Some(ast::OnNoMatch::TextOnlyCopy) | None => ir::ModeOnNoMatch::TextOnlyCopy,
                },
                on_multiple_match: match mode.on_multiple_match.as_ref() {
                    Some(ast::OnMultipleMatch::Fail) => ir::OnMultipleMatch::Fail,
                    Some(ast::OnMultipleMatch::UseLast) | None => ir::OnMultipleMatch::UseLast,
                },
                warning_on_no_match: mode.warning_on_no_match,
                typed: match mode.typed.as_ref() {
                    Some(ast::Typed::Yes) => ir::ModeTyped::Yes,
                    Some(ast::Typed::Strict) => ir::ModeTyped::Strict,
                    Some(ast::Typed::Lax) => ir::ModeTyped::Lax,
                    Some(ast::Typed::No) | Some(ast::Typed::Unspecified) | None => {
                        ir::ModeTyped::No
                    }
                },
            },
        );
        Ok(())
    }

    fn output(
        &mut self,
        declarations: &mut ir::Declarations,
        output: &ast::Output,
    ) -> error::SpannedResult<()> {
        if output.name.is_some() {
            return Ok(());
        }
        let serialization = &mut declarations.serialization_params;
        if output.parameter_document.is_some() {
            return Err(error::Error::Unsupported(String::from(
                "Output: Parameter documents are not supported yet",
            ))
            .into());
        }
        if output.build_tree {
            return Err(error::Error::Unsupported(String::from(
                "Output: Build tree is not supported yet",
            ))
            .into());
        }
        fn assign_if_some<T>(location: &mut T, value: Option<T>) {
            if let Some(v) = value {
                *location = v;
            }
        }
        serialization.allow_duplicate_names = output.allow_duplicate_names;
        serialization.byte_order_mark = output.byte_order_mark;
        serialization
            .cdata_section_elements
            .extend(output.cdata_section_elements.clone());
        serialization.doctype_public = output.doctype_public.clone();
        serialization.doctype_system = output.doctype_system.clone();
        match &output.method {
            Some(ast::OutputMethod::Adaptive) => {
                serialization.method = QNameOrString::String("adaptive".to_string())
            }
            Some(ast::OutputMethod::Xml) => {
                serialization.method = QNameOrString::String("xml".to_string())
            }
            Some(ast::OutputMethod::Html) => {
                serialization.method = QNameOrString::String("html".to_string())
            }
            Some(ast::OutputMethod::Xhtml) => {
                serialization.method = QNameOrString::String("xhtml".to_string())
            }
            Some(ast::OutputMethod::Text) => {
                serialization.method = QNameOrString::String("text".to_string())
            }
            Some(ast::OutputMethod::Json) => {
                serialization.method = QNameOrString::String("json".to_string())
            }
            None => {}
            method => {
                return Err(error::Error::Unsupported(format!(
                    "Output method {:?} not supported yet",
                    method
                ))
                .into());
            }
        };
        assign_if_some(&mut serialization.encoding, output.encoding.clone());
        serialization.escape_uri_attributes = output.escape_uri_attributes;
        assign_if_some(&mut serialization.html_version, output.html_version);
        serialization.explicit_html_version = output.html_version.is_some();
        serialization.include_content_type = output.include_content_type;
        serialization.indent = output.indent;
        assign_if_some(
            &mut serialization.item_separator,
            output.item_separator.clone(),
        );
        match &output.json_node_output_method {
            Some(ast::JsonNodeOutputMethod::Xml) => {
                serialization.json_node_output_method = QNameOrString::String("xml".to_string())
            }
            Some(ast::JsonNodeOutputMethod::Html) => {
                serialization.json_node_output_method = QNameOrString::String("html".to_string())
            }
            None => {}
            method => {
                return Err(error::Error::Unsupported(format!(
                    "JSON node output method {:?} not supported yet",
                    method
                ))
                .into());
            }
        }
        serialization.media_type = output.media_type.clone();
        serialization.normalization_form =
            output.normalization_form.as_ref().and_then(|nf| match nf {
                ast::NormalizationForm::Nfc => Some(String::from("NFC")),
                ast::NormalizationForm::Nfd => Some(String::from("NFD")),
                ast::NormalizationForm::Nfkc => Some(String::from("NFKC")),
                ast::NormalizationForm::Nfkd => Some(String::from("NFKD")),
                ast::NormalizationForm::FullyNormalized => Some(String::from("fully-normalized")),
                ast::NormalizationForm::NmToken(nm) => Some(nm.clone()),
                ast::NormalizationForm::None => None,
            });
        serialization.omit_xml_declaration = output.omit_xml_declaration;
        assign_if_some(
            &mut serialization.standalone,
            output.standalone.as_ref().map(|s| match s {
                ast::Standalone::Bool(b) => Some(*b),
                ast::Standalone::Omit => None,
            }),
        );
        serialization
            .suppress_indentation
            .extend(output.suppress_indentation.clone());
        serialization.undeclare_prefixes = output.undeclare_prefixes;
        if !output.use_character_maps.is_empty() {
            serialization.use_character_maps =
                self.resolve_character_maps(&output.use_character_maps)?;
        }
        assign_if_some(&mut serialization.version, output.version.clone());
        Ok(())
    }

    fn ast_mode_value_to_ir_mode_value(mode: &ast::ModeValue) -> ir::ModeValue {
        match mode {
            ast::ModeValue::EqName(name) => ir::ModeValue::Named(name.clone()),
            ast::ModeValue::Unnamed => ir::ModeValue::Unnamed,
            ast::ModeValue::All => ir::ModeValue::All,
        }
    }

    fn sequence_constructor_function(
        &mut self,
        sequence_constructor: &ast::SequenceConstructor,
    ) -> error::SpannedResult<ir::FunctionDefinition> {
        let context_names = self.variables.push_context();
        let bindings = self.sequence_constructor(sequence_constructor)?;
        self.variables.pop_context();
        let params = vec![
            ir::Param {
                name: context_names.item,
                type_: None,
                default: None,
                required: false,
                original_name: None,
                tunnel: false,
            },
            ir::Param {
                name: context_names.position,
                type_: None,
                default: None,
                required: false,
                original_name: None,
                tunnel: false,
            },
            ir::Param {
                name: context_names.last,
                type_: None,
                default: None,
                required: false,
                original_name: None,
                tunnel: false,
            },
        ];
        Ok(ir::FunctionDefinition {
            params,
            return_type: None,
            body: Box::new(bindings.expr()),
        })
    }

    fn sequence_constructor(
        &mut self,
        sequence_constructor: &[ast::SequenceConstructorItem],
    ) -> error::SpannedResult<Bindings> {
        self.variables.push_scope();
        let result = self.sequence_constructor_in_scope(sequence_constructor);
        self.variables.pop_scope();
        result
    }

    fn sequence_constructor_in_scope(
        &mut self,
        sequence_constructor: &[ast::SequenceConstructorItem],
    ) -> error::SpannedResult<Bindings> {
        let mut items = sequence_constructor.iter();
        let left = items.next();
        if let Some(left) = left {
            if let Some((name, var_bindings)) = self.variable(left)? {
                let expr = ir::Expr::Let(ir::Let {
                    name,
                    var_expr: Box::new(var_bindings.expr()),
                    return_expr: Box::new(
                        self.sequence_constructor_in_scope(items.as_slice())?.expr(),
                    ),
                });
                return Ok(Bindings::new(
                    self.variables.new_binding(expr, (0..0).into()),
                ));
            }

            let mut left_bindings = self.sequence_constructor_item(left)?;
            if items.as_slice().is_empty() {
                return Ok(left_bindings);
            }
            let mut right_bindings = self.sequence_constructor_in_scope(items.as_slice())?;
            let expr = ir::Expr::Binary(ir::Binary {
                left: left_bindings.atom(),
                op: ir::BinaryOperator::Comma,
                right: right_bindings.atom(),
            });
            let binding = self.variables.new_binding_no_span(expr);
            Ok(left_bindings.concat(right_bindings).bind(binding))
        } else {
            let empty_sequence = self.empty_sequence();
            Ok(Bindings::new(
                self.variables
                    .new_binding(empty_sequence.value, empty_sequence.span),
            ))
        }
    }

    fn sequence_constructor_item(
        &mut self,
        item: &ast::SequenceConstructorItem,
    ) -> error::SpannedResult<Bindings> {
        match item {
            ast::SequenceConstructorItem::Instruction(instruction) => {
                self.sequence_constructor_instruction(instruction)
            }
            ast::SequenceConstructorItem::Content(content) => {
                self.sequence_constructor_content(content)
            }
        }
    }

    fn sequence_constructor_instruction(
        &mut self,
        instruction: &ast::SequenceConstructorInstruction,
    ) -> error::SpannedResult<Bindings> {
        use ast::SequenceConstructorInstruction::*;
        match instruction {
            ApplyTemplates(apply_templates) => self.apply_templates(apply_templates),
            ApplyImports(apply_imports) => self.apply_imports(apply_imports),
            CallTemplate(call_template) => self.call_template(call_template),
            PerformSort(perform_sort) => self.perform_sort(perform_sort),
            ValueOf(value_of) => self.value_of(value_of),
            If(if_) => self.if_(if_),
            Choose(choose) => self.choose(choose),
            ForEach(for_each) => self.for_each(for_each),
            ForEachGroup(for_each_group) => self.for_each_group(for_each_group),
            Merge(merge) => self.merge(merge),
            Iterate(iterate) => self.iterate(iterate),
            NextIteration(next_iteration) => self.next_iteration(next_iteration),
            NextMatch(next_match) => self.next_match(next_match),
            Break(break_) => self.break_(break_),
            Copy(copy) => self.copy(copy),
            CopyOf(copy_of) => self.copy_of(copy_of),
            Message(message) => self.message(message),
            ResultDocument(result_document) => self.result_document(result_document),
            Sequence(sequence) => self.sequence(sequence),
            SourceDocument(source_document) => self.source_document(source_document),
            Document(document) => self.document(document),
            Element(element) => self.element(element),
            Text(text) => self.text(text),
            Try(try_) => self.try_(try_),
            Evaluate(evaluate) => self.evaluate(evaluate),
            Number(number) => self.number(number),
            Map(map) => self.map(map),
            Attribute(attribute) => self.attribute(attribute),
            Namespace(namespace) => self.namespace(namespace),
            Comment(comment) => self.comment(comment),
            ProcessingInstruction(pi) => self.processing_instruction(pi),
            Fallback(_) => {
                let empty_sequence = self.empty_sequence();
                Ok(Bindings::new(
                    self.variables.new_binding_no_span(empty_sequence.value),
                ))
            }
            // TODO: xsl:variable does not produce content and is handled
            // earlier already should be unreachable!() but at this point this
            // can be reached so return unsupported
            Variable(_variable) => Err(error::Error::Unsupported(String::from(
                "Internal bug: variable node should have been processed already",
            ))
            .into()),
            MapEntry(entry) => self.map_entry(entry),
            AnalyzeString(analyze_string) => self.analyze_string(analyze_string),
            WherePopulated(where_populated) => self.where_populated(where_populated),
            _ => Err(error::Error::Unsupported(format!(
                "Instruction not supported: {:?}",
                instruction
            ))
            .into()),
        }
    }

    fn map(&mut self, map: &ast::Map) -> error::SpannedResult<Bindings> {
        // Compile the body as a sequence constructor — each xsl:map-entry
        // produces a singleton map, other instructions can too.
        let body_bindings = self.sequence_constructor(&map.sequence_constructor)?;
        let (body_atom, body_bindings) = body_bindings.atom_bindings();

        // Wrap with map:merge to combine the singleton maps
        let merge_expr = self.static_function_call_expr(
            "merge",
            "http://www.w3.org/2005/xpath-functions/map",
            1,
            vec![body_atom],
        );

        Ok(body_bindings.bind_expr(
            &mut self.variables,
            Spanned::new(merge_expr, (map.span.start..map.span.end).into()),
        ))
    }

    fn map_entry(&mut self, entry: &ast::MapEntry) -> error::SpannedResult<Bindings> {
        let (key_atom, key_bindings) = self.expression(&entry.key)?.atom_bindings();
        let value_bindings = if let Some(select) = &entry.select {
            self.expression(select)?
        } else {
            self.sequence_constructor(&entry.sequence_constructor)?
        };
        let (value_atom, value_bindings) = value_bindings.atom_bindings();

        let entry_expr = self.static_function_call_expr(
            "entry",
            "http://www.w3.org/2005/xpath-functions/map",
            2,
            vec![key_atom, value_atom],
        );

        let bindings = key_bindings.concat(value_bindings);
        Ok(bindings.bind_expr(
            &mut self.variables,
            Spanned::new(entry_expr, (entry.span.start..entry.span.end).into()),
        ))
    }

    fn analyze_string(
        &mut self,
        analyze_string: &ast::AnalyzeString,
    ) -> error::SpannedResult<Bindings> {
        // Compile select expression
        let (select_atom, select_bindings) =
            self.expression(&analyze_string.select)?.atom_bindings();
        let select_str_expr =
            self.static_function_call_expr("string", FN_NAMESPACE, 1, vec![select_atom]);
        let (input_atom, input_bindings) = select_bindings
            .bind_expr_no_span(&mut self.variables, select_str_expr)
            .atom_bindings();

        // Compile regex AVT
        let regex_bindings = self.attribute_value_template(&analyze_string.regex)?;
        let (regex_atom, regex_bindings) = regex_bindings.atom_bindings();

        // Compile flags AVT (default empty string)
        let (flags_atom, flags_bindings) = if let Some(flags) = &analyze_string.flags {
            let fb = self.attribute_value_template(flags)?;
            fb.atom_bindings()
        } else {
            let empty = Spanned::new(
                ir::Atom::Const(ir::Const::String(String::new())),
                (0..0).into(),
            );
            (empty, Bindings::empty())
        };

        // Compile matching-substring body as a closure(xs:string) -> item()*
        let (match_fn_atom, match_fn_bindings) =
            if let Some(matching) = &analyze_string.matching_substring {
                self.analyze_string_closure(&matching.sequence_constructor)?
            } else {
                self.empty_closure()?
            };

        // Compile non-matching-substring body as a closure(xs:string) -> item()*
        let (non_match_fn_atom, non_match_fn_bindings) =
            if let Some(non_matching) = &analyze_string.non_matching_substring {
                self.analyze_string_closure(&non_matching.sequence_constructor)?
            } else {
                self.empty_closure()?
            };

        let bindings = input_bindings
            .concat(regex_bindings)
            .concat(flags_bindings)
            .concat(match_fn_bindings)
            .concat(non_match_fn_bindings);

        let expr = self.static_function_call_expr(
            "xslt-analyze-string",
            FN_NAMESPACE,
            5,
            vec![
                input_atom,
                regex_atom,
                flags_atom,
                match_fn_atom,
                non_match_fn_atom,
            ],
        );

        Ok(bindings.bind_expr(
            &mut self.variables,
            Spanned::new(
                expr,
                (analyze_string.span.start..analyze_string.span.end).into(),
            ),
        ))
    }

    /// Create a closure that takes a string parameter and evaluates the body
    /// with that string as the context item.
    fn analyze_string_closure(
        &mut self,
        sequence_constructor: &[ast::SequenceConstructorItem],
    ) -> error::SpannedResult<(ir::AtomS, Bindings)> {
        let param_name = self.variables.new_name();
        let context_names = self.variables.push_context();
        let body_bindings = self.with_template_continuation_availability(false, |this| {
            this.sequence_constructor(sequence_constructor)
        })?;
        self.variables.pop_context();

        let body = ir::Expr::Map(ir::Map {
            context_names,
            var_atom: Spanned::new(ir::Atom::Variable(param_name.clone()), (0..0).into()),
            return_expr: Box::new(body_bindings.expr()),
        });

        let body_bindings = Bindings::empty().bind_expr_no_span(&mut self.variables, body);
        Ok(self.closure(
            vec![ir::Param {
                name: param_name,
                type_: None,
                default: None,
                required: false,
                original_name: None,
                tunnel: false,
            }],
            body_bindings,
        ))
    }

    /// Create an empty closure that returns the empty sequence.
    fn empty_closure(&mut self) -> error::SpannedResult<(ir::AtomS, Bindings)> {
        let param_name = self.variables.new_name();
        let empty = self.empty_sequence();
        let body = Bindings::new(self.variables.new_binding_no_span(empty.value));
        Ok(self.closure(
            vec![ir::Param {
                name: param_name,
                type_: None,
                default: None,
                required: false,
                original_name: None,
                tunnel: false,
            }],
            body,
        ))
    }

    fn where_populated(
        &mut self,
        where_populated: &ast::WherePopulated,
    ) -> error::SpannedResult<Bindings> {
        let (body_atom, body_bindings) = self
            .sequence_constructor(&where_populated.sequence_constructor)?
            .atom_bindings();

        let expr = self.static_function_call_expr(
            "xslt-where-populated",
            FN_NAMESPACE,
            1,
            vec![body_atom],
        );
        Ok(body_bindings.bind_expr(
            &mut self.variables,
            Spanned::new(
                expr,
                (where_populated.span.start..where_populated.span.end).into(),
            ),
        ))
    }

    fn evaluate(&mut self, evaluate: &ast::Evaluate) -> error::SpannedResult<Bindings> {
        if evaluate.schema_aware.is_some() {
            return Err(error::Error::Unsupported(format!(
                "Instruction not supported: {:?}",
                evaluate
            ))
            .into());
        }

        let (xpath_value_atom, xpath_value_bindings) =
            self.expression(&evaluate.xpath)?.atom_bindings();
        let xpath_expr = self.simple_content_expr(xpath_value_atom, self.space_separator_atom());
        let (xpath_atom, xpath_bindings) = xpath_value_bindings
            .bind_expr_no_span(&mut self.variables, xpath_expr)
            .atom_bindings();

        let (context_item_atom, context_item_bindings) =
            self.optional_evaluate_argument(evaluate.context_item.as_ref())?;
        let (context_item_supplied_atom, context_item_supplied_bindings) =
            if evaluate.context_item.is_some() {
                (
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String("supplied".to_string())),
                        (0..0).into(),
                    ),
                    Bindings::empty(),
                )
            } else {
                let empty_sequence = self.empty_sequence();
                Bindings::empty()
                    .bind_expr_no_span(&mut self.variables, empty_sequence.value)
                    .atom_bindings()
            };
        let xpath_default_namespace_atom = Spanned::new(
            ir::Atom::Const(ir::Const::String(
                evaluate.static_xpath_default_namespace.clone(),
            )),
            (0..0).into(),
        );
        let default_collation_atom = Spanned::new(
            ir::Atom::Const(ir::Const::String(
                evaluate
                    .static_default_collation
                    .first()
                    .cloned()
                    .unwrap_or_else(|| {
                        self.current_static_context()
                            .default_collation_uri()
                            .to_string()
                    }),
            )),
            (0..0).into(),
        );
        let (namespace_context_atom, namespace_context_bindings) =
            self.optional_evaluate_argument(evaluate.namespace_context.as_ref())?;
        let (with_params_atom, with_params_bindings) =
            self.evaluate_with_params_argument(evaluate)?;
        let (base_uri_atom, base_uri_bindings) =
            self.optional_evaluate_base_uri_argument(evaluate.base_uri.as_ref())?;

        let expr = self.static_function_call_expr(
            "xslt-evaluate",
            FN_NAMESPACE,
            8,
            vec![
                xpath_atom,
                context_item_atom,
                context_item_supplied_atom,
                xpath_default_namespace_atom,
                default_collation_atom,
                namespace_context_atom,
                with_params_atom,
                base_uri_atom,
            ],
        );
        let bindings = xpath_bindings
            .concat(context_item_bindings)
            .concat(context_item_supplied_bindings)
            .concat(namespace_context_bindings)
            .concat(with_params_bindings)
            .concat(base_uri_bindings)
            .bind_expr(
                &mut self.variables,
                Spanned::new(expr, (evaluate.span.start..evaluate.span.end).into()),
            );

        self.convert_bindings(bindings, evaluate.as_.as_ref(), RaisedError::XPTY0004)
    }

    fn evaluate_with_params_argument(
        &mut self,
        evaluate: &ast::Evaluate,
    ) -> error::SpannedResult<(ir::AtomS, Bindings)> {
        let (mut with_params_atom, mut with_params_bindings) =
            self.optional_evaluate_argument(evaluate.with_params.as_ref())?;

        for item in &evaluate.content {
            let ast::EvaluateContent::WithParam(with_param) = item else {
                continue;
            };

            let (param, value_bindings) = self.evaluate_with_param(with_param)?;
            let value_atom = param
                .select
                .expect("xsl:with-param lowering should always yield a select atom");
            let (key_atom, key_bindings) = self.xml_name(&with_param.name)?.atom_bindings();

            let put_if_absent = self.static_function_call_expr(
                "xslt-evaluate-put-param",
                FN_NAMESPACE,
                3,
                vec![with_params_atom, key_atom, value_atom],
            );

            let (next_atom, next_bindings) = with_params_bindings
                .concat(key_bindings)
                .concat(value_bindings)
                .bind_expr_no_span(&mut self.variables, put_if_absent)
                .atom_bindings();

            with_params_atom = next_atom;
            with_params_bindings = next_bindings;
        }

        Ok((with_params_atom, with_params_bindings))
    }

    fn optional_evaluate_base_uri_argument(
        &mut self,
        value_template: Option<&ast::ValueTemplate<ast::Uri>>,
    ) -> error::SpannedResult<(ir::AtomS, Bindings)> {
        if let Some(value_template) = value_template {
            return Ok(self
                .attribute_value_template(value_template)?
                .atom_bindings());
        }

        let empty_sequence = self.empty_sequence();
        Ok(Bindings::empty()
            .bind_expr_no_span(&mut self.variables, empty_sequence.value)
            .atom_bindings())
    }

    fn optional_evaluate_argument(
        &mut self,
        expression: Option<&ast::Expression>,
    ) -> error::SpannedResult<(ir::AtomS, Bindings)> {
        if let Some(expression) = expression {
            return Ok(self.expression(expression)?.atom_bindings());
        }

        let empty_sequence = self.empty_sequence();
        Ok(Bindings::empty()
            .bind_expr_no_span(&mut self.variables, empty_sequence.value)
            .atom_bindings())
    }

    fn number(&mut self, number: &ast::Number) -> error::SpannedResult<Bindings> {
        // Reject unsupported formatting attributes early
        if number.lang.is_some()
            || number.letter_value.is_some()
            || number.ordinal.is_some()
            || number.start_at.is_some()
            || number.grouping_separator.is_some()
            || number.grouping_size.is_some()
        {
            return Err(error::Error::Unsupported(format!(
                "Instruction not supported: {:?}",
                number
            ))
            .into());
        }

        // Compile the format AVT (shared between value and counting forms)
        let (format_atom, format_bindings) = self.number_format(&number.format)?;

        if let Some(value) = &number.value {
            // value= form: evaluate expression, format, emit text
            let (value_atom, value_bindings) = self.expression(value)?.atom_bindings();
            let string_expr = self.static_function_call_expr(
                "xslt-number-value",
                FN_NAMESPACE,
                2,
                vec![value_atom, format_atom],
            );
            let (text_atom, bindings) = value_bindings
                .concat(format_bindings)
                .bind_expr_no_span(&mut self.variables, string_expr)
                .atom_bindings();
            Ok(bindings.bind_expr_no_span(
                &mut self.variables,
                ir::Expr::XmlText(ir::XmlText { value: text_atom }),
            ))
        } else {
            // Counting form
            let level = number.level.as_ref().unwrap_or(&ast::NumberLevel::Single);

            // Compile the selected node (select= or context item)
            let (node_atom, node_bindings) = if let Some(select) = &number.select {
                self.expression(select)?.atom_bindings()
            } else {
                self.variables.context_item((0..0).into())?.atom_bindings()
            };

            match level {
                ast::NumberLevel::Single | ast::NumberLevel::Any => {
                    if number.count.is_some() || number.from.is_some() {
                        // Compile count/from patterns and pass indices to runtime
                        let count_index: i64 = if let Some(count_pattern) = &number.count {
                            self.compile_number_pattern(count_pattern)? as i64
                        } else {
                            -1 // sentinel: use default count
                        };
                        let from_index: i64 = if let Some(from_pattern) = &number.from {
                            self.compile_number_pattern(from_pattern)? as i64
                        } else {
                            -1 // sentinel: no from boundary
                        };

                        let (count_index_atom, count_index_bindings) = Bindings::empty()
                            .bind_expr_no_span(
                                &mut self.variables,
                                ir::Expr::Atom(Spanned::new(
                                    ir::Atom::Const(ir::Const::Integer(count_index.into())),
                                    (0..0).into(),
                                )),
                            )
                            .atom_bindings();
                        let (from_index_atom, from_index_bindings) = Bindings::empty()
                            .bind_expr_no_span(
                                &mut self.variables,
                                ir::Expr::Atom(Spanned::new(
                                    ir::Atom::Const(ir::Const::Integer(from_index.into())),
                                    (0..0).into(),
                                )),
                            )
                            .atom_bindings();

                        let fn_name = match level {
                            ast::NumberLevel::Single => "xslt-number-count-single-pattern",
                            ast::NumberLevel::Any => "xslt-number-count-any-pattern",
                            _ => unreachable!(),
                        };

                        let string_expr = self.static_function_call_expr(
                            fn_name,
                            FN_NAMESPACE,
                            4,
                            vec![node_atom, count_index_atom, from_index_atom, format_atom],
                        );
                        let (text_atom, bindings) = node_bindings
                            .concat(format_bindings)
                            .concat(count_index_bindings)
                            .concat(from_index_bindings)
                            .bind_expr_no_span(&mut self.variables, string_expr)
                            .atom_bindings();
                        Ok(bindings.bind_expr_no_span(
                            &mut self.variables,
                            ir::Expr::XmlText(ir::XmlText { value: text_atom }),
                        ))
                    } else {
                        // Default count/from: use the simpler runtime function
                        let fn_name = match level {
                            ast::NumberLevel::Single => "xslt-number-count-single",
                            ast::NumberLevel::Any => "xslt-number-count-any",
                            _ => unreachable!(),
                        };
                        let string_expr = self.static_function_call_expr(
                            fn_name,
                            FN_NAMESPACE,
                            2,
                            vec![node_atom, format_atom],
                        );
                        let (text_atom, bindings) = node_bindings
                            .concat(format_bindings)
                            .bind_expr_no_span(&mut self.variables, string_expr)
                            .atom_bindings();
                        Ok(bindings.bind_expr_no_span(
                            &mut self.variables,
                            ir::Expr::XmlText(ir::XmlText { value: text_atom }),
                        ))
                    }
                }
                ast::NumberLevel::Multiple => {
                    if number.count.is_some() || number.from.is_some() {
                        let count_index: i64 = if let Some(count_pattern) = &number.count {
                            self.compile_number_pattern(count_pattern)? as i64
                        } else {
                            -1
                        };
                        let from_index: i64 = if let Some(from_pattern) = &number.from {
                            self.compile_number_pattern(from_pattern)? as i64
                        } else {
                            -1
                        };

                        let (count_index_atom, count_index_bindings) = Bindings::empty()
                            .bind_expr_no_span(
                                &mut self.variables,
                                ir::Expr::Atom(Spanned::new(
                                    ir::Atom::Const(ir::Const::Integer(count_index.into())),
                                    (0..0).into(),
                                )),
                            )
                            .atom_bindings();
                        let (from_index_atom, from_index_bindings) = Bindings::empty()
                            .bind_expr_no_span(
                                &mut self.variables,
                                ir::Expr::Atom(Spanned::new(
                                    ir::Atom::Const(ir::Const::Integer(from_index.into())),
                                    (0..0).into(),
                                )),
                            )
                            .atom_bindings();

                        let string_expr = self.static_function_call_expr(
                            "xslt-number-count-multiple-pattern",
                            FN_NAMESPACE,
                            4,
                            vec![node_atom, count_index_atom, from_index_atom, format_atom],
                        );
                        let (text_atom, bindings) = node_bindings
                            .concat(format_bindings)
                            .concat(count_index_bindings)
                            .concat(from_index_bindings)
                            .bind_expr_no_span(&mut self.variables, string_expr)
                            .atom_bindings();
                        Ok(bindings.bind_expr_no_span(
                            &mut self.variables,
                            ir::Expr::XmlText(ir::XmlText { value: text_atom }),
                        ))
                    } else {
                        let string_expr = self.static_function_call_expr(
                            "xslt-number-count-multiple",
                            FN_NAMESPACE,
                            2,
                            vec![node_atom, format_atom],
                        );
                        let (text_atom, bindings) = node_bindings
                            .concat(format_bindings)
                            .bind_expr_no_span(&mut self.variables, string_expr)
                            .atom_bindings();
                        Ok(bindings.bind_expr_no_span(
                            &mut self.variables,
                            ir::Expr::XmlText(ir::XmlText { value: text_atom }),
                        ))
                    }
                }
            }
        }
    }

    fn number_format(
        &mut self,
        format: &Option<ast::ValueTemplate<String>>,
    ) -> error::SpannedResult<(Spanned<ir::Atom>, Bindings)> {
        if let Some(format) = format {
            Ok(self.attribute_value_template(format)?.atom_bindings())
        } else {
            let bindings = Bindings::empty();
            let format_expr = ir::Expr::Atom(Spanned::new(
                ir::Atom::Const(ir::Const::String("1".to_string())),
                (0..0).into(),
            ));
            Ok(bindings
                .bind_expr_no_span(&mut self.variables, format_expr)
                .atom_bindings())
        }
    }

    fn message(&mut self, message: &ast::Message) -> error::SpannedResult<Bindings> {
        let empty_sequence = self.empty_sequence();
        let message_bindings = if let Some(select) = &message.select {
            self.expression(select)?
        } else if !message.sequence_constructor.is_empty() {
            if self.processor_xslt_version() < 3 {
                self.sequence_constructor_with_temporary_output_state(&message.sequence_constructor)?
            } else {
                self.sequence_constructor(&message.sequence_constructor)?
            }
        } else {
            Bindings::new(
                self.variables
                    .new_binding_no_span(empty_sequence.value.clone()),
            )
        };

        // Check if terminate is set
        let should_terminate = if let Some(terminate) = &message.terminate {
            if let Some(value) = self.static_value_template(terminate) {
                matches!(value.trim(), "yes" | "true" | "1")
            } else {
                false
            }
        } else {
            false
        };

        if should_terminate {
            // Check for custom error-code
            let custom_error = if let Some(error_code) = &message.error_code {
                if let Some(code_str) = self.static_value_template(error_code) {
                    self.resolve_eqname_string(&code_str, &message.namespaces)
                } else {
                    None
                }
            } else {
                None
            };

            let error_bindings = if let Some((local_name, namespace, prefix)) = custom_error {
                let ns_atom =
                    Spanned::new(ir::Atom::Const(ir::Const::String(namespace)), (0..0).into());
                let local_atom = Spanned::new(
                    ir::Atom::Const(ir::Const::String(local_name)),
                    (0..0).into(),
                );
                let prefix_atom =
                    Spanned::new(ir::Atom::Const(ir::Const::String(prefix)), (0..0).into());
                let call_expr = self.static_function_call_expr(
                    "xslt-message-terminate",
                    FN_NAMESPACE,
                    3,
                    vec![ns_atom, local_atom, prefix_atom],
                );
                Bindings::empty().bind_expr_no_span(&mut self.variables, call_expr)
            } else {
                self.raise_error(RaisedError::XTMM9000)
            };
            Ok(message_bindings.concat(error_bindings))
        } else {
            // Non-terminate: output message content to stderr via xslt-message
            let (message_atom, bindings) = message_bindings.atom_bindings();
            let call_expr =
                self.static_function_call_expr("xslt-message", FN_NAMESPACE, 1, vec![message_atom]);
            Ok(bindings.bind_expr_no_span(&mut self.variables, call_expr))
        }
    }

    /// Resolve an EQName string (Q{ns}local or prefix:local or local) into
    /// (local_name, namespace, prefix) triple.
    fn resolve_eqname_string(
        &self,
        s: &str,
        namespaces: &[ast::LiteralNamespace],
    ) -> Option<(String, String, String)> {
        let s = s.trim();
        // Handle Q{namespace}local-name syntax
        if let Some(rest) = s.strip_prefix("Q{") {
            if let Some(close_brace) = rest.find('}') {
                let namespace = &rest[..close_brace];
                let local_name = &rest[close_brace + 1..];
                if !local_name.is_empty() {
                    return Some((local_name.to_string(), namespace.to_string(), String::new()));
                }
            }
            return None;
        }
        // Handle prefix:local-name syntax
        if let Some((prefix, local_name)) = s.split_once(':') {
            // Check literal namespaces first (from the containing element)
            let namespace = namespaces
                .iter()
                .find(|ns| ns.prefix == prefix)
                .map(|ns| ns.uri.as_str())
                .or_else(|| self.current_static_context().namespaces().by_prefix(prefix))?;
            return Some((
                local_name.to_string(),
                namespace.to_string(),
                prefix.to_string(),
            ));
        }
        // Plain local name — no namespace
        Some((s.to_string(), String::new(), String::new()))
    }

    fn result_document(
        &mut self,
        result_document: &ast::ResultDocument,
    ) -> error::SpannedResult<Bindings> {
        if matches!(
            result_document.validation,
            Some(ast::Validation::Strict | ast::Validation::Lax | ast::Validation::Preserve)
        ) || result_document.type_.is_some()
            || result_document.allow_duplicate_names.is_some()
            || result_document.escape_uri_attributes.is_some()
            || result_document.json_node_output_method.is_some()
            || result_document.normalization_form.is_some()
            || result_document.parameter_document.is_some()
            || result_document.suppress_indentation.is_some()
        {
            return Err(error::Error::Unsupported(String::from(
                "xsl:result-document serialization attributes are not supported yet",
            ))
            .into());
        }

        if let Some(build_tree) = &result_document.build_tree {
            let build_tree_literal = self
                .static_value_template(build_tree)
                .ok_or_else(|| error::SpannedError {
                    error: error::Error::Unsupported(
                        "Dynamic xsl:result-document @build-tree is not supported yet"
                            .to_string(),
                    ),
                    span: Some((result_document.span.start..result_document.span.end).into()),
                })?;
            let build_tree_literal = Self::validate_boolean_literal(&build_tree_literal)?;
            if matches!(build_tree_literal.as_str(), "yes" | "true" | "1") {
                return Err(error::Error::Unsupported(String::from(
                    "xsl:result-document @build-tree='yes' is not supported yet",
                ))
                .into());
            }
            if result_document.href.is_some() {
                return Err(error::Error::Unsupported(String::from(
                    "xsl:result-document @build-tree='no' with @href is not supported yet",
                ))
                .into());
            }
        }

        let (content_atom, content_bindings) = if result_document.sequence_constructor.is_empty() {
            (
                Spanned::new(ir::Atom::Const(ir::Const::EmptySequence), (0..0).into()),
                Bindings::empty(),
            )
        } else {
            if result_document.href.is_some() {
                self.secondary_result_document_depth += 1;
                let result = self
                    .sequence_constructor(&result_document.sequence_constructor)?
                    .atom_bindings();
                self.secondary_result_document_depth -= 1;
                result
            } else {
                self.sequence_constructor(&result_document.sequence_constructor)?
                    .atom_bindings()
            }
        };

        let (formatted_output, format_atom, format_bindings, format_namespaces_atom, named_outputs_atom) =
            match &result_document.format {
                Some(format) if self.static_value_template(format).is_some() => (
                    Some(self.resolve_named_output(format, &result_document.namespaces)?),
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String(String::new())),
                        (0..0).into(),
                    ),
                    Bindings::empty(),
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String(String::new())),
                        (0..0).into(),
                    ),
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String(String::new())),
                        (0..0).into(),
                    ),
                ),
                Some(format) => {
                    let (format_atom, format_bindings) =
                        self.attribute_value_template(format)?.atom_bindings();
                    (
                        None,
                        format_atom,
                        format_bindings,
                        Spanned::new(
                            ir::Atom::Const(ir::Const::String(
                                self.encode_literal_namespaces(&result_document.namespaces),
                            )),
                            (0..0).into(),
                        ),
                        Spanned::new(
                            ir::Atom::Const(ir::Const::String(self.encode_named_outputs()?)),
                            (0..0).into(),
                        ),
                    )
                }
                None => (
                    None,
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String(String::new())),
                        (0..0).into(),
                    ),
                    Bindings::empty(),
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String(String::new())),
                        (0..0).into(),
                    ),
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String(String::new())),
                        (0..0).into(),
                    ),
                ),
            };

        let (method_atom, method_bindings) = if let Some(method) = &result_document.method {
            if let Some(method_literal) = self.static_value_template(method) {
                (
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String(method_literal)),
                        (0..0).into(),
                    ),
                    Bindings::empty(),
                )
            } else {
                self.attribute_value_template(method)?.atom_bindings()
            }
        } else if let Some(output) = formatted_output.as_ref() {
            (
                Spanned::new(
                    ir::Atom::Const(ir::Const::String(
                        Self::output_method_literal(output.method.as_ref())?.unwrap_or_default(),
                    )),
                    (0..0).into(),
                ),
                Bindings::empty(),
            )
        } else {
            (
                Spanned::new(
                    ir::Atom::Const(ir::Const::String(String::new())),
                    (0..0).into(),
                ),
                Bindings::empty(),
            )
        };

        let (byte_order_mark_atom, byte_order_mark_bindings) =
            self.validated_value_template_or_literal_atom(
                result_document.bye_order_mark.as_ref(),
                formatted_output
                    .as_ref()
                    .map(|output| output.byte_order_mark.to_string())
                    .unwrap_or_default(),
                Self::validate_boolean_literal,
            )?;

        let (cdata_atom, cdata_bindings) = if let Some(cdata_section_elements) =
            &result_document.cdata_section_elements
        {
            if let Some(direct_literal) = self.static_value_template(cdata_section_elements) {
                let cdata_literal = if let Some(output) = formatted_output.as_ref() {
                    Self::merge_literal_qname_lists(
                        &output.cdata_section_elements,
                        Some(direct_literal),
                    )
                } else {
                    direct_literal
                };
                (
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String(cdata_literal)),
                        (0..0).into(),
                    ),
                    Bindings::empty(),
                )
            } else {
                let dynamic_cdata = self.attribute_value_template(cdata_section_elements)?;
                if let Some(output) = formatted_output.as_ref() {
                    let static_cdata = Self::qname_list_literal(&output.cdata_section_elements);
                    if static_cdata.is_empty() {
                        dynamic_cdata.atom_bindings()
                    } else {
                        let (dynamic_cdata_atom, dynamic_cdata_bindings) =
                            dynamic_cdata.atom_bindings();
                        let expr = ir::Expr::FunctionCall(ir::FunctionCall {
                            atom: Spanned::new(self.concat_atom(2), (0..0).into()),
                            args: vec![
                                Spanned::new(
                                    ir::Atom::Const(ir::Const::String(format!(
                                        "{static_cdata} "
                                    ))),
                                    (0..0).into(),
                                ),
                                dynamic_cdata_atom,
                            ],
                        });
                        dynamic_cdata_bindings
                            .bind_expr_no_span(&mut self.variables, expr)
                            .atom_bindings()
                    }
                } else {
                    dynamic_cdata.atom_bindings()
                }
            }
        } else if let Some(output) = formatted_output.as_ref() {
            (
                Spanned::new(
                    ir::Atom::Const(ir::Const::String(Self::qname_list_literal(
                        &output.cdata_section_elements,
                    ))),
                    (0..0).into(),
                ),
                Bindings::empty(),
            )
        } else {
            (
                Spanned::new(
                    ir::Atom::Const(ir::Const::String(String::new())),
                    (0..0).into(),
                ),
                Bindings::empty(),
            )
        };

        let (doctype_public_atom, doctype_public_bindings) = self.value_template_or_literal_atom(
            result_document.doctype_public.as_ref(),
            formatted_output
                .as_ref()
                .and_then(|output| output.doctype_public.clone())
                .unwrap_or_default(),
        )?;

        let (doctype_system_atom, doctype_system_bindings) = self.value_template_or_literal_atom(
            result_document.doctype_system.as_ref(),
            formatted_output
                .as_ref()
                .and_then(|output| output.doctype_system.clone())
                .unwrap_or_default(),
        )?;

        let (include_content_type_atom, include_content_type_bindings) =
            self.validated_value_template_or_literal_atom(
                result_document.include_content_type.as_ref(),
                formatted_output
                    .as_ref()
                    .map(|output| output.include_content_type.to_string())
                    .unwrap_or_default(),
                Self::validate_boolean_literal,
            )?;

        let (media_type_atom, media_type_bindings) = self.value_template_or_literal_atom(
            result_document.media_type.as_ref(),
            formatted_output
                .as_ref()
                .and_then(|output| output.media_type.clone())
                .unwrap_or_default(),
        )?;

        let (item_separator_atom, item_separator_bindings) = if let Some(item_separator) =
            &result_document.item_separator
        {
            if let Some(item_separator_literal) = self.static_value_template(item_separator) {
                (
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String(item_separator_literal)),
                        (0..0).into(),
                    ),
                    Bindings::empty(),
                )
            } else {
                self.attribute_value_template(item_separator)?.atom_bindings()
            }
        } else if let Some(output) = formatted_output.as_ref() {
            (
                Spanned::new(
                    ir::Atom::Const(ir::Const::String(
                        output.item_separator.clone().unwrap_or_default(),
                    )),
                    (0..0).into(),
                ),
                Bindings::empty(),
            )
        } else {
            (
                Spanned::new(
                    ir::Atom::Const(ir::Const::String(String::new())),
                    (0..0).into(),
                ),
                Bindings::empty(),
            )
        };

        let (omit_xml_declaration_atom, omit_xml_declaration_bindings) =
            self.validated_value_template_or_literal_atom(
                result_document.omit_xml_declaration.as_ref(),
                formatted_output
                    .as_ref()
                    .map(|output| output.omit_xml_declaration.to_string())
                    .unwrap_or_default(),
                Self::validate_boolean_literal,
            )?;

        let (standalone_atom, standalone_bindings) =
            self.validated_value_template_or_literal_atom(
                result_document.standalone.as_ref(),
                formatted_output
                    .as_ref()
                    .and_then(|output| Self::output_standalone_literal(output.standalone.as_ref()))
                    .unwrap_or_default(),
                Self::validate_standalone_literal,
            )?;

        let (html_version_atom, html_version_bindings) = if let Some(html_version) =
            &result_document.html_version
        {
            if let Some(html_version_literal) = self.static_value_template(html_version) {
                rust_decimal::Decimal::from_str_exact(&html_version_literal).map_err(|_| {
                    error::SpannedError {
                        error: error::Error::XTSE0020,
                        span: Some((result_document.span.start..result_document.span.end).into()),
                    }
                })?;
                (
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String(html_version_literal)),
                        (0..0).into(),
                    ),
                    Bindings::empty(),
                )
            } else {
                self.attribute_value_template(html_version)?.atom_bindings()
            }
        } else if let Some(output) = formatted_output.as_ref() {
            (
                Spanned::new(
                    ir::Atom::Const(ir::Const::String(
                        output
                            .html_version
                            .map(|html_version| html_version.to_string())
                            .unwrap_or_default(),
                    )),
                    (0..0).into(),
                ),
                Bindings::empty(),
            )
        } else {
            (
                Spanned::new(
                    ir::Atom::Const(ir::Const::String(String::new())),
                    (0..0).into(),
                ),
                Bindings::empty(),
            )
        };

        let use_character_maps_literal = if let Some(use_character_maps) =
            &result_document.use_character_maps
        {
            let mut merged_character_maps = if let Some(output) = formatted_output.as_ref() {
                self.resolve_character_maps(&output.use_character_maps)?
            } else {
                ahash::HashMap::default()
            };
            for (character, replacement) in self.resolve_character_maps(use_character_maps)? {
                merged_character_maps.insert(character, replacement);
            }
            Self::encode_character_maps(&merged_character_maps)
        } else if let Some(output) = formatted_output.as_ref() {
            Self::encode_character_maps(&self.resolve_character_maps(&output.use_character_maps)?)
        } else {
            String::new()
        };
        let use_character_maps_atom = Spanned::new(
            ir::Atom::Const(ir::Const::String(use_character_maps_literal)),
            (0..0).into(),
        );

        let (version_atom, version_bindings) = self.value_template_or_literal_atom(
            result_document.version.as_ref(),
            formatted_output
                .as_ref()
                .and_then(|output| output.version.clone())
                .unwrap_or_default(),
        )?;

        if let Some(href) = &result_document.href {
            let (href_atom, href_bindings) = self.attribute_value_template(href)?.atom_bindings();
            let bindings = href_bindings
                .concat(content_bindings)
                .concat(format_bindings)
                .concat(method_bindings)
                .concat(byte_order_mark_bindings)
                .concat(cdata_bindings)
                .concat(doctype_public_bindings)
                .concat(doctype_system_bindings)
                .concat(include_content_type_bindings)
                .concat(media_type_bindings)
                .concat(item_separator_bindings)
                .concat(omit_xml_declaration_bindings)
                .concat(standalone_bindings)
                .concat(html_version_bindings)
                .concat(version_bindings);
            let expr = self.static_function_call_expr(
                "store-result-document",
                FN_NAMESPACE,
                18,
                vec![
                    href_atom,
                    content_atom,
                    format_atom,
                    format_namespaces_atom,
                    named_outputs_atom,
                    method_atom,
                    byte_order_mark_atom,
                    cdata_atom,
                    doctype_public_atom,
                    doctype_system_atom,
                    include_content_type_atom,
                    media_type_atom,
                    item_separator_atom,
                    omit_xml_declaration_atom,
                    standalone_atom,
                    html_version_atom,
                    use_character_maps_atom,
                    version_atom,
                ],
            );
            return Ok(bindings.bind_expr(
                &mut self.variables,
                Spanned::new(
                    expr,
                    (result_document.span.start..result_document.span.end).into(),
                ),
            ));
        }

        let expr = self.static_function_call_expr(
            "store-principal-result-document",
            FN_NAMESPACE,
            17,
            vec![
                content_atom,
                format_atom,
                format_namespaces_atom,
                named_outputs_atom,
                method_atom,
                byte_order_mark_atom,
                cdata_atom,
                doctype_public_atom,
                doctype_system_atom,
                include_content_type_atom,
                media_type_atom,
                item_separator_atom,
                omit_xml_declaration_atom,
                standalone_atom,
                html_version_atom,
                use_character_maps_atom,
                version_atom,
            ],
        );
        Ok(content_bindings
            .concat(format_bindings)
            .concat(method_bindings)
            .concat(byte_order_mark_bindings)
            .concat(cdata_bindings)
            .concat(doctype_public_bindings)
            .concat(doctype_system_bindings)
            .concat(include_content_type_bindings)
            .concat(media_type_bindings)
            .concat(item_separator_bindings)
            .concat(omit_xml_declaration_bindings)
            .concat(standalone_bindings)
            .concat(html_version_bindings)
            .concat(version_bindings)
            .bind_expr(
                &mut self.variables,
                Spanned::new(
                    expr,
                    (result_document.span.start..result_document.span.end).into(),
                ),
            ))
    }

    fn sequence_constructor_content(
        &mut self,
        content: &ast::Content,
    ) -> error::SpannedResult<Bindings> {
        match content {
            ast::Content::Element(element_node) => {
                self.sequence_constructor_content_element(element_node)
            }
            ast::Content::Text(text) => {
                let text_atom = Spanned::new(
                    ir::Atom::Const(ir::Const::String(text.clone())),
                    (0..0).into(),
                );
                let bindings = Bindings::empty();
                Ok(bindings.bind_expr_no_span(
                    &mut self.variables,
                    ir::Expr::XmlText(ir::XmlText { value: text_atom }),
                ))
            }
            ast::Content::Value(expression) => {
                let (atom, bindings) = self.expression(expression)?.atom_bindings();
                let expr = self.simple_content_expr(atom, self.space_separator_atom());
                let (text_atom, bindings) = bindings
                    .bind_expr_no_span(&mut self.variables, expr)
                    .atom_bindings();
                Ok(bindings.bind_expr_no_span(
                    &mut self.variables,
                    ir::Expr::XmlText(ir::XmlText { value: text_atom }),
                ))
            }
        }
    }

    fn sequence_constructor_content_element(
        &mut self,
        element_node: &ast::ElementNode,
    ) -> error::SpannedResult<Bindings> {
        let element_name = self.apply_namespace_alias(&element_node.name);
        let aliased_attributes = element_node
            .attributes
            .iter()
            .map(|(name, value)| (self.apply_namespace_alias(name), value.clone()))
            .collect::<Vec<_>>();

        let (name_atom, bindings) = self.xml_name(&element_name)?.atom_bindings();
        let name_expr = ir::Expr::XmlElement(ir::XmlElement { name: name_atom });
        let (element_atom, mut bindings) = bindings
            .bind_expr_no_span(&mut self.variables, name_expr)
            .atom_bindings();
        let mut namespaces = element_node.namespaces.clone();
        Self::ensure_name_namespace(&mut namespaces, &element_name);
        for (name, _) in &aliased_attributes {
            Self::ensure_name_namespace(&mut namespaces, name);
        }
        for namespace in &namespaces {
            let prefix_atom = Spanned::new(
                ir::Atom::Const(ir::Const::String(namespace.prefix.clone())),
                (0..0).into(),
            );
            let namespace_atom = Spanned::new(
                ir::Atom::Const(ir::Const::String(namespace.uri.clone())),
                (0..0).into(),
            );
            let namespace_expr = ir::Expr::XmlNamespace(ir::XmlNamespace {
                prefix: prefix_atom,
                namespace: namespace_atom,
            });
            let (namespace_atom, namespace_bindings) = Bindings::empty()
                .bind_expr_no_span(&mut self.variables, namespace_expr)
                .atom_bindings();
            let append_expr = ir::Expr::XmlAppend(ir::XmlAppend {
                parent: element_atom.clone(),
                child: namespace_atom,
            });
            let append_bindings =
                namespace_bindings.bind_expr_no_span(&mut self.variables, append_expr);
            bindings = bindings.concat(append_bindings);
        }
        let attribute_set_bindings = self.attribute_set_append(
            element_atom.clone(),
            element_node.use_attribute_sets.as_deref().unwrap_or(&[]),
        )?;
        bindings = bindings.concat(attribute_set_bindings);
        for (name, value) in &aliased_attributes {
            let (value_atom, value_bindings) =
                self.attribute_value_template(value)?.atom_bindings();
            let (attribute_name_atom, attribute_bindings) = self.xml_name(name)?.atom_bindings();
            let value_bindings = value_bindings.concat(attribute_bindings);
            let attribute_expr = ir::Expr::XmlAttribute(ir::XmlAttribute {
                name: attribute_name_atom,
                value: value_atom,
            });
            let (attribute_atom, attribute_bindings) = value_bindings
                .bind_expr_no_span(&mut self.variables, attribute_expr)
                .atom_bindings();
            let append_expr = ir::Expr::XmlAppend(ir::XmlAppend {
                parent: element_atom.clone(),
                child: attribute_atom,
            });
            let append_bindings =
                attribute_bindings.bind_expr_no_span(&mut self.variables, append_expr);
            bindings = bindings.concat(append_bindings);
        }
        let sequence_constructor_bindings = self.sequence_constructor_append(
            element_atom.clone(),
            &element_node.sequence_constructor,
        )?;
        let bindings = bindings.concat(sequence_constructor_bindings);
        Ok(bindings)
    }

    fn attribute_set_append(
        &mut self,
        element_atom: ir::AtomS,
        use_attribute_sets: &[ast::EqName],
    ) -> error::SpannedResult<Bindings> {
        if use_attribute_sets.is_empty() {
            return Ok(Bindings::empty());
        }

        let (atom, bindings) = self
            .attribute_set_bindings(use_attribute_sets)?
            .atom_bindings();
        let append = ir::Expr::XmlAppend(ir::XmlAppend {
            parent: element_atom,
            child: atom,
        });
        Ok(bindings.bind_expr_no_span(&mut self.variables, append))
    }

    fn attribute_set_bindings(
        &mut self,
        use_attribute_sets: &[ast::EqName],
    ) -> error::SpannedResult<Bindings> {
        self.attribute_set_bindings_with_active(use_attribute_sets)
    }

    fn attribute_set_bindings_with_active(
        &mut self,
        use_attribute_sets: &[ast::EqName],
    ) -> error::SpannedResult<Bindings> {
        self.with_attribute_set_variable_scope(|this| {
            let mut combined: Option<Bindings> = None;

            for name in use_attribute_sets {
                let key = Self::attribute_set_key(name);
                if this.active_attribute_sets.contains(&key) {
                    return Err(error::Error::XTDE0640.into());
                }

                let attribute_sets = this
                    .attribute_sets
                    .get(&key)
                    .cloned()
                    .ok_or(error::Error::XTSE0710)?;

                this.active_attribute_sets.push(key);
                for attribute_set in attribute_sets {
                    let attribute_set_base_uri = attribute_set
                        .xml_base
                        .as_deref()
                        .and_then(|uri| this.resolve_static_base_uri(uri));
                    this.with_static_base_uri(attribute_set_base_uri, |this| {
                        if let Some(nested) = &attribute_set.use_attribute_sets {
                            let nested_bindings =
                                this.attribute_set_bindings_with_active(nested)?;
                            combined = Some(match combined.take() {
                                Some(existing) => this.comma_bindings(existing, nested_bindings),
                                None => nested_bindings,
                            });
                        }
                        for attribute in &attribute_set.attributes {
                            let attribute_bindings = this.attribute(attribute)?;
                            combined = Some(match combined.take() {
                                Some(existing) => this.comma_bindings(existing, attribute_bindings),
                                None => attribute_bindings,
                            });
                        }
                        Ok(())
                    })?;
                }
                this.active_attribute_sets.pop();
            }

            Ok(combined.unwrap_or_else(|| {
                let empty_sequence = this.empty_sequence();
                Bindings::empty().bind_expr_no_span(&mut this.variables, empty_sequence.value)
            }))
        })
    }

    fn comma_bindings(&mut self, left: Bindings, right: Bindings) -> Bindings {
        let mut left = left;
        let mut right = right;
        let expr = ir::Expr::Binary(ir::Binary {
            left: left.atom(),
            op: ir::BinaryOperator::Comma,
            right: right.atom(),
        });
        let binding = self.variables.new_binding_no_span(expr);
        left.concat(right).bind(binding)
    }

    fn ensure_name_namespace(namespaces: &mut Vec<ast::LiteralNamespace>, name: &ast::Name) {
        if name.namespace().is_empty() {
            return;
        }

        let prefix = name.prefix().to_string();
        let uri = name.namespace().to_string();
        let already_declared = namespaces
            .iter()
            .any(|namespace| namespace.prefix == prefix && namespace.uri == uri);
        if !already_declared {
            namespaces.push(ast::LiteralNamespace { prefix, uri });
        }
    }

    fn sequence_constructor_append(
        &mut self,
        element_atom: ir::AtomS,
        sequence_constructor: &ast::SequenceConstructor,
    ) -> error::SpannedResult<Bindings> {
        if !sequence_constructor.is_empty() {
            let (atom, bindings) = self
                .sequence_constructor(sequence_constructor)?
                .atom_bindings();
            let append = ir::Expr::XmlAppend(ir::XmlAppend {
                parent: element_atom,
                child: atom,
            });
            let bindings = bindings.bind_expr_no_span(&mut self.variables, append);
            Ok(bindings)
        } else {
            Ok(Bindings::empty())
        }
    }

    fn space_separator_atom(&self) -> ir::AtomS {
        Spanned::new(
            ir::Atom::Const(ir::Const::String(" ".to_string())),
            (0..0).into(),
        )
    }

    fn apply_templates(
        &mut self,
        apply_templates: &ast::ApplyTemplates,
    ) -> error::SpannedResult<Bindings> {
        let (select_atom, bindings) = self.expression(&apply_templates.select)?.atom_bindings();
        let mut sorts = Vec::new();
        let mut params = Vec::new();
        let mut param_bindings = Bindings::empty();

        for content in &apply_templates.content {
            match content {
                ast::ApplyTemplatesContent::Sort(sort) => sorts.push(sort),
                ast::ApplyTemplatesContent::WithParam(with_param) => {
                    let (param, select_bindings) = self.with_param(with_param)?;
                    param_bindings = param_bindings.concat(select_bindings);
                    params.push(param);
                }
            }
        }

        let (select_atom, sort_bindings) = self.apply_template_sorts(select_atom, &sorts)?;

        let mode = match &apply_templates.mode {
            ast::ApplyTemplatesModeValue::EqName(name) => {
                ir::ApplyTemplatesModeValue::Named(name.clone())
            }
            ast::ApplyTemplatesModeValue::Unnamed => ir::ApplyTemplatesModeValue::Unnamed,
            ast::ApplyTemplatesModeValue::Current => {
                if self.variables.current_context_names().is_none() {
                    ir::ApplyTemplatesModeValue::Unnamed
                } else {
                    ir::ApplyTemplatesModeValue::Current
                }
            }
        };

        let bindings = bindings.concat(sort_bindings).concat(param_bindings);

        Ok(bindings.bind_expr_no_span(
            &mut self.variables,
            ir::Expr::ApplyTemplates(ir::ApplyTemplates {
                mode,
                select: select_atom,
                builtin_template_params_passthrough: apply_templates
                    .builtin_template_params_passthrough,
                params,
            }),
        ))
    }

    fn apply_imports(
        &mut self,
        apply_imports: &ast::ApplyImports,
    ) -> error::SpannedResult<Bindings> {
        self.continue_template(
            apply_imports.with_params.iter(),
            ir::ContinueBehavior::ApplyImports,
        )
    }

    fn next_match(&mut self, next_match: &ast::NextMatch) -> error::SpannedResult<Bindings> {
        self.continue_template(
            next_match
                .content
                .iter()
                .filter_map(|content| match content {
                    ast::NextMatchContent::WithParam(with_param) => Some(with_param),
                    ast::NextMatchContent::Fallback(_) => None,
                }),
            ir::ContinueBehavior::NextMatch,
        )
    }

    fn continue_template<'b>(
        &mut self,
        with_params: impl Iterator<Item = &'b ast::WithParam>,
        behavior: ir::ContinueBehavior,
    ) -> error::SpannedResult<Bindings> {
        if !self.template_continuation_available {
            return Ok(self.raise_error(RaisedError::XTDE0560));
        }

        let mut params = Vec::new();
        let mut param_bindings = Bindings::empty();

        for with_param in with_params {
            let (param, select_bindings) = self.with_param(with_param)?;
            param_bindings = param_bindings.concat(select_bindings);
            params.push(param);
        }

        Ok(param_bindings.bind_expr_no_span(
            &mut self.variables,
            ir::Expr::ContinueTemplate(ir::ContinueTemplate { params, behavior }),
        ))
    }

    fn apply_template_sorts(
        &mut self,
        select_atom: ir::AtomS,
        sorts: &[&ast::Sort],
    ) -> error::SpannedResult<(ir::AtomS, Bindings)> {
        let mut current_atom = select_atom;
        let mut bindings = Bindings::empty();

        for sort in sorts.iter().rev() {
            self.ensure_supported_sort(sort)?;
            let (collation_atom, collation_bindings) = self.sort_collation_atom(sort)?;

            bindings = bindings.concat(collation_bindings);
            let sort_expr = if self.sort_uses_default_text_key(sort)? {
                self.static_function_call_expr(
                    "sort",
                    FN_NAMESPACE,
                    2,
                    vec![current_atom.clone(), collation_atom],
                )
            } else {
                let (key_atom, key_bindings) = self.sort_key_function(sort)?;
                bindings = bindings.concat(key_bindings);
                self.static_function_call_expr(
                    "sort",
                    FN_NAMESPACE,
                    3,
                    vec![current_atom.clone(), collation_atom, key_atom],
                )
            };
            let (sorted_atom, sorted_bindings) = bindings
                .bind_expr_no_span(&mut self.variables, sort_expr)
                .atom_bindings();
            bindings = sorted_bindings;
            current_atom = sorted_atom;

            if self.sort_is_descending(sort)? {
                let reverse_expr = self.static_function_call_expr(
                    "reverse",
                    FN_NAMESPACE,
                    1,
                    vec![current_atom.clone()],
                );
                let (reversed_atom, reversed_bindings) = bindings
                    .bind_expr_no_span(&mut self.variables, reverse_expr)
                    .atom_bindings();
                bindings = reversed_bindings;
                current_atom = reversed_atom;
            }
        }

        Ok((current_atom, bindings))
    }

    fn sort_uses_default_text_key(&self, sort: &ast::Sort) -> error::SpannedResult<bool> {
        Ok(sort.select.is_none()
            && sort.sequence_constructor.is_empty()
            && matches!(self.sort_data_type(sort)?, SortDataType::Text))
    }

    fn sort_key_function(
        &mut self,
        sort: &ast::Sort,
    ) -> error::SpannedResult<(ir::AtomS, Bindings)> {
        let param_name = self.variables.new_name();
        let context_names = self.variables.push_context();
        let return_bindings = self.sort_key_return_bindings(sort)?;
        self.variables.pop_context();

        let body = ir::Expr::Map(ir::Map {
            context_names,
            var_atom: Spanned::new(ir::Atom::Variable(param_name.clone()), (0..0).into()),
            return_expr: Box::new(return_bindings.expr()),
        });
        let function = ir::Expr::FunctionDefinition(ir::FunctionDefinition {
            params: vec![ir::Param {
                name: param_name,
                type_: None,
                default: None,
                required: false,
                original_name: None,
                tunnel: false,
            }],
            return_type: None,
            body: Box::new(Spanned::new(body, (0..0).into())),
        });

        let bindings = Bindings::empty().bind_expr_no_span(&mut self.variables, function);
        Ok(bindings.atom_bindings())
    }

    fn sort_key_return_bindings(&mut self, sort: &ast::Sort) -> error::SpannedResult<Bindings> {
        let key_bindings = if let Some(select) = &sort.select {
            self.expression(select)?
        } else if !sort.sequence_constructor.is_empty() {
            self.sequence_constructor_with_temporary_output_state(&sort.sequence_constructor)?
        } else {
            self.variables.context_item((0..0).into())?
        };
        self.atomized_key_bindings(key_bindings, self.sort_data_type(sort)?)
    }

    fn atomized_key_bindings(
        &mut self,
        key_bindings: Bindings,
        data_type: SortDataType,
    ) -> error::SpannedResult<Bindings> {
        let (key_atom, bindings) = key_bindings.atom_bindings();
        let atomized_expr = self.static_function_call_expr("data", FN_NAMESPACE, 1, vec![key_atom]);
        let bindings = bindings.bind_expr_no_span(&mut self.variables, atomized_expr);

        match data_type {
            SortDataType::Text => Ok(bindings),
            SortDataType::Number => {
                let (atomized_atom, bindings) = bindings.atom_bindings();
                let number_expr =
                    self.static_function_call_expr("number", FN_NAMESPACE, 1, vec![atomized_atom]);
                Ok(bindings.bind_expr_no_span(&mut self.variables, number_expr))
            }
        }
    }

    fn sort_collation_atom(
        &mut self,
        sort: &ast::Sort,
    ) -> error::SpannedResult<(ir::AtomS, Bindings)> {
        let case_first_suffix = self.sort_case_order_suffix(sort)?;
        if let Some(collation) = &sort.collation {
            if let Some(suffix) = &case_first_suffix {
                // Append caseFirst to the explicit collation URI
                let base = self
                    .literal_value_template(collation, "xsl:sort collation")?
                    .unwrap_or_default();
                let sep = if base.contains('?') { ";" } else { "?" };
                let uri = format!("{}{}{}", base, sep, suffix);
                Ok((
                    Spanned::new(ir::Atom::Const(ir::Const::String(uri)), (0..0).into()),
                    Bindings::empty(),
                ))
            } else {
                Ok(self.attribute_value_template(collation)?.atom_bindings())
            }
        } else if let Some(suffix) = &case_first_suffix {
            let uri = format!("http://www.w3.org/2013/collation/UCA?{}", suffix);
            Ok((
                Spanned::new(ir::Atom::Const(ir::Const::String(uri)), (0..0).into()),
                Bindings::empty(),
            ))
        } else {
            Ok((
                Spanned::new(ir::Atom::Const(ir::Const::EmptySequence), (0..0).into()),
                Bindings::empty(),
            ))
        }
    }

    fn sort_case_order_suffix(&self, sort: &ast::Sort) -> error::SpannedResult<Option<String>> {
        let Some(case_order) = &sort.case_order else {
            return Ok(None);
        };
        match self
            .literal_value_template(case_order, "xsl:sort case-order")?
            .as_deref()
        {
            Some("upper-first") => Ok(Some("caseFirst=upper".to_string())),
            Some("lower-first") => Ok(Some("caseFirst=lower".to_string())),
            Some(value) => Err(error::Error::Unsupported(format!(
                "xsl:sort case-order value {:?} is not supported",
                value
            ))
            .into()),
            None => Ok(None),
        }
    }

    fn ensure_supported_sort(&self, _sort: &ast::Sort) -> error::SpannedResult<()> {
        // lang attribute is accepted but silently ignored (uses default Unicode collation)
        Ok(())
    }

    fn sort_is_descending(&self, sort: &ast::Sort) -> error::SpannedResult<bool> {
        let Some(order) = &sort.order else {
            return Ok(false);
        };
        match self
            .literal_value_template(order, "xsl:sort order")?
            .as_deref()
        {
            Some("ascending") => Ok(false),
            Some("descending") => Ok(true),
            Some(value) => Err(error::Error::Unsupported(format!(
                "xsl:sort order value {:?} is not supported yet",
                value
            ))
            .into()),
            None => Ok(false),
        }
    }

    fn sort_data_type(&self, sort: &ast::Sort) -> error::SpannedResult<SortDataType> {
        let Some(data_type) = &sort.data_type else {
            return Ok(SortDataType::Text);
        };
        match self
            .literal_value_template(data_type, "xsl:sort data-type")?
            .as_deref()
        {
            Some("text") => Ok(SortDataType::Text),
            Some("number") => Ok(SortDataType::Number),
            Some(value) => Err(error::Error::Unsupported(format!(
                "xsl:sort data-type value {:?} is not supported yet",
                value
            ))
            .into()),
            None => Ok(SortDataType::Text),
        }
    }

    fn literal_value_template<V>(
        &self,
        value_template: &ast::ValueTemplate<V>,
        attribute: &str,
    ) -> error::SpannedResult<Option<String>>
    where
        V: Clone + PartialEq + Eq,
    {
        let mut value = String::new();
        for item in &value_template.template {
            match item {
                ast::ValueTemplateItem::String { text, .. } => value.push_str(text),
                ast::ValueTemplateItem::Curly { c } => value.push(*c),
                ast::ValueTemplateItem::Value { .. } => {
                    return Err(error::Error::Unsupported(format!(
                        "{} AVTs are not supported yet",
                        attribute
                    ))
                    .into())
                }
            }
        }
        Ok(Some(value))
    }

    fn call_template(
        &mut self,
        call_template: &ast::CallTemplate,
    ) -> error::SpannedResult<Bindings> {
        // Compile the with-params for the template invocation
        let mut params = Vec::new();
        let mut param_bindings = Bindings::empty();

        for with_param in &call_template.with_params {
            let (param, select_bindings) = self.with_param(with_param)?;
            param_bindings = param_bindings.concat(select_bindings);
            params.push(param);
        }

        let call_template_expr = ir::Expr::CallTemplate(ir::CallTemplate {
            name: ir::Name::new(call_template.name.local_name().to_string()),
            context: if self
                .named_templates_with_absent_context
                .contains(call_template.name.local_name())
            {
                None
            } else {
                self.variables.current_context_names()
            },
            backwards_compatible: call_template.backwards_compatible,
            params,
        });

        Ok(param_bindings.bind_expr_no_span(&mut self.variables, call_template_expr))
    }

    fn zero_arg_closure(&mut self, body: Bindings) -> (ir::AtomS, Bindings) {
        self.closure(Vec::new(), body)
    }

    fn closure(&mut self, params: Vec<ir::Param>, body: Bindings) -> (ir::AtomS, Bindings) {
        let function_definition = ir::FunctionDefinition {
            params,
            return_type: None,
            body: Box::new(body.expr()),
        };
        let function_expr = Bindings::empty().bind_expr_no_span(
            &mut self.variables,
            ir::Expr::FunctionDefinition(function_definition),
        );
        function_expr.atom_bindings()
    }

    fn catch_error_params() -> Vec<ir::Param> {
        [
            "code",
            "description",
            "value",
            "module",
            "line-number",
            "column-number",
        ]
        .into_iter()
        .map(|name| ir::Param {
            name: ir::Name::new(name.to_string()),
            type_: None,
            default: None,
            required: false,
            original_name: None,
            tunnel: false,
        })
        .collect()
    }

    fn catch_error_variable_names() -> Vec<Name> {
        [
            "code",
            "description",
            "value",
            "module",
            "line-number",
            "column-number",
        ]
        .into_iter()
        .map(|name| {
            Name::new(
                name.to_string(),
                "http://www.w3.org/2005/xqt-errors".to_string(),
                "err".to_string(),
            )
        })
        .collect()
    }

    fn catch_handler_closure(
        &mut self,
        catch: &ast::Catch,
    ) -> error::SpannedResult<(ir::AtomS, Bindings)> {
        let params = Self::catch_error_params();
        let variable_names = Self::catch_error_variable_names();

        self.variables.push_scope();
        for (variable_name, param) in variable_names.into_iter().zip(params.iter()) {
            self.variables
                .insert_var_name_in_current_scope(variable_name, param.name.clone());
        }
        let body = self.select_or_sequence_constructor(catch);
        self.variables.pop_scope();

        Ok(self.closure(params, body?))
    }

    fn square_array_atom(&mut self, atoms: Vec<ir::AtomS>) -> (ir::AtomS, Bindings) {
        let array_expr = Bindings::empty().bind_expr_no_span(
            &mut self.variables,
            ir::Expr::ArrayConstructor(ir::ArrayConstructor::Square(atoms)),
        );
        array_expr.atom_bindings()
    }

    fn try_fallback_bindings(&mut self, try_: &ast::Try) -> error::SpannedResult<Bindings> {
        let fallbacks = try_
            .catches
            .iter()
            .filter_map(|catch_or_fallback| match catch_or_fallback {
                ast::TryCatchOrFallback::Fallback(fallback) => Some(fallback),
                ast::TryCatchOrFallback::Catch(_) => None,
            })
            .flat_map(|fallback| fallback.sequence_constructor.iter().cloned())
            .collect::<Vec<_>>();

        self.sequence_constructor(&fallbacks)
    }

    fn try_has_prior_output_before_source_document(&self, try_: &ast::Try) -> bool {
        let mut saw_prior_output = false;
        for item in &try_.sequence_constructor {
            match item {
                ast::SequenceConstructorItem::Instruction(
                    ast::SequenceConstructorInstruction::SourceDocument(_),
                ) => return saw_prior_output,
                ast::SequenceConstructorItem::Instruction(
                    ast::SequenceConstructorInstruction::Variable(_),
                ) => {}
                _ => saw_prior_output = true,
            }
        }
        false
    }

    fn try_(&mut self, try_: &ast::Try) -> error::SpannedResult<Bindings> {
        let processor_xslt_version = self.static_context.processor_xslt_version().unwrap_or(3);
        if processor_xslt_version < 3 {
            if try_.xslt_version > processor_xslt_version {
                return self.try_fallback_bindings(try_);
            }
            return Err(
                error::Error::XTSE0010.with_ast_span((try_.span.start..try_.span.end).into())
            );
        }

        let try_body = self.select_or_sequence_constructor(try_)?;
        let (body_atom, body_bindings) = self.zero_arg_closure(try_body);
        let mut bindings = body_bindings;
        let mut handler_atoms = Vec::new();
        let mut pattern_atoms = Vec::new();

        let catches = std::iter::once(&try_.catch).chain(try_.catches.iter().filter_map(
            |catch_or_fallback| match catch_or_fallback {
                ast::TryCatchOrFallback::Catch(catch) => Some(catch),
                ast::TryCatchOrFallback::Fallback(_) => None,
            },
        ));

        for catch in catches {
            let (handler_atom, handler_bindings) = self.catch_handler_closure(catch)?;
            bindings = bindings.concat(handler_bindings);
            handler_atoms.push(handler_atom);

            let pattern = catch
                .errors
                .as_ref()
                .filter(|errors| !errors.is_empty())
                .map(|errors| errors.join(" "))
                .unwrap_or_else(|| "*".to_string());
            pattern_atoms.push(Spanned::new(
                ir::Atom::Const(ir::Const::String(pattern)),
                (0..0).into(),
            ));
        }

        let (patterns_atom, pattern_bindings) = self.square_array_atom(pattern_atoms);
        let (handlers_atom, handler_bindings) = self.square_array_atom(handler_atoms);
        bindings = bindings.concat(pattern_bindings).concat(handler_bindings);
        let rollback_output_atom = Spanned::new(
            ir::Atom::Const(ir::Const::String(
                if try_.rollback_output.unwrap_or(true) {
                    "yes"
                } else {
                    "no"
                }
                .to_string(),
            )),
            (0..0).into(),
        );
        let nonrecoverable_on_error_atom = Spanned::new(
            ir::Atom::Const(ir::Const::String(
                if !try_.rollback_output.unwrap_or(true)
                    && self.try_has_prior_output_before_source_document(try_)
                {
                    "yes"
                } else {
                    "no"
                }
                .to_string(),
            )),
            (0..0).into(),
        );

        let expr = self.static_function_call_expr(
            "xslt-try",
            FN_NAMESPACE,
            5,
            vec![
                body_atom,
                patterns_atom,
                handlers_atom,
                rollback_output_atom,
                nonrecoverable_on_error_atom,
            ],
        );
        Ok(bindings.bind_expr_no_span(&mut self.variables, expr))
    }

    fn select_or_sequence_constructor(
        &mut self,
        instruction: &impl ast::SelectOrSequenceConstructor,
    ) -> error::SpannedResult<Bindings> {
        if let Some(select) = instruction.select() {
            self.expression(select)
        } else {
            self.sequence_constructor(instruction.sequence_constructor())
        }
    }

    fn select_or_sequence_constructor_simple_content(
        &mut self,
        instruction: &impl ast::SelectOrSequenceConstructor,
    ) -> error::SpannedResult<Bindings> {
        let (select_atom, bindings) = self
            .select_or_sequence_constructor(instruction)?
            .atom_bindings();

        let separator_atom = self.space_separator_atom();
        let expr = self.simple_content_expr(select_atom, separator_atom);
        Ok(bindings.bind_expr_no_span(&mut self.variables, expr))
    }

    fn select_or_sequence_constructor_simple_content_with_separator(
        &mut self,
        instruction: &impl ast::SelectOrSequenceConstructor,
        separator: &Option<ast::ValueTemplate<String>>,
    ) -> error::SpannedResult<Bindings> {
        let (select_atom, select_bindings) = self
            .select_or_sequence_constructor(instruction)?
            .atom_bindings();

        let (separator_atom, separator_bindings) = if let Some(separator) = separator {
            self.attribute_value_template(separator)?
        } else {
            Bindings::new(
                self.variables
                    .new_binding_no_span(ir::Expr::Atom(self.space_separator_atom())),
            )
        }
        .atom_bindings();
        let bindings = select_bindings.concat(separator_bindings);
        let expr = self.simple_content_expr(select_atom, separator_atom);
        Ok(bindings.bind_expr_no_span(&mut self.variables, expr))
    }

    fn value_of(&mut self, value_of: &ast::ValueOf) -> error::SpannedResult<Bindings> {
        let content_bindings = if self.processor_xslt_version() < 3
            && value_of.select.is_none()
            && !value_of.sequence_constructor.is_empty()
        {
            let value_bindings = self.select_or_sequence_constructor_with_temporary_output_state(value_of)?;
            let (value_atom, value_bindings) = value_bindings.atom_bindings();
            let (separator_atom, separator_bindings) = if let Some(separator) = &value_of.separator {
                self.attribute_value_template(separator)?
            } else {
                Bindings::new(
                    self.variables
                        .new_binding_no_span(ir::Expr::Atom(self.space_separator_atom())),
                )
            }
            .atom_bindings();
            let bindings = value_bindings.concat(separator_bindings);
            let expr = self.simple_content_expr(value_atom, separator_atom);
            bindings.bind_expr_no_span(&mut self.variables, expr)
        } else {
            self.select_or_sequence_constructor_simple_content_with_separator(
                value_of,
                &value_of.separator,
            )?
        };
        let (text_atom, bindings) = content_bindings.atom_bindings();

        Ok(bindings.bind_expr_no_span(
            &mut self.variables,
            ir::Expr::XmlText(ir::XmlText { value: text_atom }),
        ))
    }

    fn attribute_value_template<V>(
        &mut self,
        value_template: &ast::ValueTemplate<V>,
    ) -> error::SpannedResult<Bindings>
    where
        V: Clone + PartialEq + Eq,
    {
        let mut all_bindings = Vec::new();
        for item in &value_template.template {
            let bindings = match item {
                ast::ValueTemplateItem::String { text, span: _span } => {
                    let text_atom = Spanned::new(
                        ir::Atom::Const(ir::Const::String(text.clone())),
                        (0..0).into(),
                    );
                    let bindings = Bindings::empty();
                    bindings.bind_expr_no_span(&mut self.variables, ir::Expr::Atom(text_atom))
                }
                ast::ValueTemplateItem::Curly { c } => {
                    let text_atom = Spanned::new(
                        ir::Atom::Const(ir::Const::String(c.to_string())),
                        (0..0).into(),
                    );
                    let bindings = Bindings::empty();
                    bindings.bind_expr_no_span(&mut self.variables, ir::Expr::Atom(text_atom))
                }
                ast::ValueTemplateItem::Value { xpath, span: _ } => {
                    let (atom, bindings) = self.xpath(&xpath.0, &[])?.atom_bindings();
                    let expr = self.simple_content_expr(atom, self.space_separator_atom());
                    bindings.bind_expr_no_span(&mut self.variables, expr)
                }
            };
            all_bindings.push(bindings);
        }
        Ok(if all_bindings.is_empty() {
            // empty attribute value template is a string
            let bindings = Bindings::empty();
            let empty_string = ir::Expr::Atom(self.empty_string());
            bindings.bind_expr_no_span(&mut self.variables, empty_string)
        } else if all_bindings.len() == 1 {
            // a single binding is just that binding
            all_bindings.pop().unwrap()
        } else {
            // TODO: speculative code, needs tests
            // if we have multiple bindings, concatenate each result into
            // a single string
            let mut combined_bindings = Bindings::empty();
            let mut atoms = Vec::new();
            for binding in all_bindings {
                let (atom, binding) = binding.atom_bindings();
                combined_bindings = combined_bindings.concat(binding);
                atoms.push(atom);
            }
            // concatenate all the pieces of content into a single string
            // TODO: this may create more than we have arities for, so we may want to use more
            // generic concat function that takes a sequence at some point
            let concat_atom = self.concat_atom(atoms.len() as u8);
            let expr = ir::Expr::FunctionCall(ir::FunctionCall {
                atom: Spanned::new(concat_atom, (0..0).into()),
                args: atoms,
            });
            combined_bindings.bind_expr_no_span(&mut self.variables, expr)
        })
    }

    fn variable(
        &mut self,
        item: &ast::SequenceConstructorItem,
    ) -> error::SpannedResult<Option<(ir::Name, Bindings)>> {
        if let ast::SequenceConstructorItem::Instruction(
            ast::SequenceConstructorInstruction::Variable(variable),
        ) = item
        {
            self.validate_variable(variable)?;
            let var_bindings = if let Some(select) = &variable.select {
                self.expression(select)?
            } else if variable.as_.is_some() {
                self.sequence_constructor(&variable.sequence_constructor)?
            } else if !variable.sequence_constructor.is_empty() {
                self.temporary_tree(&variable.sequence_constructor)?
            } else {
                let empty_string = ir::Expr::Atom(self.empty_string());
                Bindings::empty().bind_expr_no_span(&mut self.variables, empty_string)
            };
            let var_bindings =
                self.convert_bindings(var_bindings, variable.as_.as_ref(), RaisedError::XTTE0570)?;
            let name = self.variables.declare_var_name(&variable.name);
            Ok(Some((name, var_bindings)))
        } else {
            Ok(None)
        }
    }

    fn temporary_tree(
        &mut self,
        sequence_constructor: &ast::SequenceConstructor,
    ) -> error::SpannedResult<Bindings> {
        let (document_atom, document_bindings) = Bindings::empty()
            .bind_expr_no_span(&mut self.variables, ir::Expr::XmlDocument(ir::XmlRoot {}))
            .atom_bindings();

        let tree_bindings = if sequence_constructor.is_empty() {
            document_bindings
        } else {
            let (child_atom, child_bindings) = self
                .sequence_constructor_with_temporary_output_state(sequence_constructor)?
                .atom_bindings();
            let append_expr = ir::Expr::XmlAppend(ir::XmlAppend {
                parent: document_atom,
                child: child_atom,
            });
            document_bindings
                .concat(child_bindings)
                .bind_expr_no_span(&mut self.variables, append_expr)
        };
        let (tree_atom, tree_bindings) = tree_bindings.atom_bindings();
        let mark_expr = self.static_function_call_expr(
            "xslt-mark-temporary-tree",
            FN_NAMESPACE,
            1,
            vec![tree_atom],
        );
        Ok(tree_bindings.bind_expr_no_span(&mut self.variables, mark_expr))
    }

    fn empty_sequence(&mut self) -> ir::ExprS {
        Spanned::new(
            ir::Expr::Atom(Spanned::new(
                ir::Atom::Const(ir::Const::EmptySequence),
                (0..0).into(),
            )),
            (0..0).into(),
        )
    }

    fn empty_string(&self) -> ir::AtomS {
        Spanned::new(
            ir::Atom::Const(ir::Const::String("".to_string())),
            (0..0).into(),
        )
    }

    fn if_(&mut self, if_: &ast::If) -> error::SpannedResult<Bindings> {
        let (condition, bindings) = self.expression(&if_.test)?.atom_bindings();
        let expr = ir::Expr::If(ir::If {
            condition,
            then: Box::new(self.sequence_constructor(&if_.sequence_constructor)?.expr()),
            else_: Box::new(self.empty_sequence()),
        });
        Ok(bindings.bind_expr_no_span(&mut self.variables, expr))
    }

    fn choose(&mut self, choose: &ast::Choose) -> error::SpannedResult<Bindings> {
        self.choose_when_otherwise(&choose.when, choose.otherwise.as_ref())
    }

    fn choose_when_otherwise(
        &mut self,
        when: &[ast::When],
        otherwise: Option<&ast::Otherwise>,
    ) -> error::SpannedResult<Bindings> {
        let first = &when.first().unwrap();
        let rest = &when[1..];

        let (condition, bindings) = self.expression(&first.test)?.atom_bindings();
        let else_expr = if !rest.is_empty() {
            self.choose_when_otherwise(rest, otherwise)?.expr()
        } else if let Some(otherwise) = otherwise {
            self.sequence_constructor(&otherwise.sequence_constructor)?
                .expr()
        } else {
            self.empty_sequence()
        };

        let expr = ir::Expr::If(ir::If {
            condition,
            then: Box::new(
                self.sequence_constructor(&first.sequence_constructor)?
                    .expr(),
            ),
            else_: Box::new(else_expr),
        });
        Ok(bindings.bind_expr_no_span(&mut self.variables, expr))
    }

    fn for_each(&mut self, for_each: &ast::ForEach) -> error::SpannedResult<Bindings> {
        let (select_atom, bindings) = self.expression(&for_each.select)?.atom_bindings();
        let sort_refs = for_each.sort.iter().collect::<Vec<_>>();
        let (var_atom, sort_bindings) = self.apply_template_sorts(select_atom, &sort_refs)?;
        let bindings = bindings.concat(sort_bindings);

        let context_names = self.variables.push_context();
        let return_bindings = self.with_template_continuation_availability(false, |this| {
            this.sequence_constructor(&for_each.sequence_constructor)
        })?;
        self.variables.pop_context();
        let expr = ir::Expr::Map(ir::Map {
            context_names,
            var_atom,
            return_expr: Box::new(return_bindings.expr()),
        });

        Ok(bindings.bind_expr_no_span(&mut self.variables, expr))
    }

    fn perform_sort(&mut self, perform_sort: &ast::PerformSort) -> error::SpannedResult<Bindings> {
        let (select_atom, bindings) = if let Some(select) = &perform_sort.select {
            self.expression(select)?.atom_bindings()
        } else {
            self.sequence_constructor_with_temporary_output_state(&perform_sort.sequence_constructor)?
                .atom_bindings()
        };
        let sort_refs = perform_sort.sorts.iter().collect::<Vec<_>>();
        let (sorted_atom, sort_bindings) = self.apply_template_sorts(select_atom, &sort_refs)?;
        let bindings = bindings.concat(sort_bindings);
        Ok(bindings.bind_expr_no_span(
            &mut self.variables,
            ir::Expr::Atom(sorted_atom),
        ))
    }

    fn merge(&mut self, merge: &ast::Merge) -> error::SpannedResult<Bindings> {
        let mut bindings = Bindings::empty();
        let mut source_atoms = Vec::new();
        let mut key_function_atoms = Vec::new();

        for merge_source in &merge.merge_sources {
            let (source_atom, source_bindings) = self.expression(&merge_source.select)?.atom_bindings();
            bindings = bindings.concat(source_bindings);
            source_atoms.push(source_atom);

            let (key_function_atom, key_function_bindings) =
                self.merge_source_key_function(merge_source)?;
            bindings = bindings.concat(key_function_bindings);
            key_function_atoms.push(key_function_atom);
        }

        let (sources_atom, source_array_bindings) = self.square_array_atom(source_atoms);
        let (key_functions_atom, key_function_array_bindings) =
            self.square_array_atom(key_function_atoms);
        bindings = bindings
            .concat(source_array_bindings)
            .concat(key_function_array_bindings);

        let expr = self.static_function_call_expr(
            "xslt-unsupported-merge",
            FN_NAMESPACE,
            2,
            vec![sources_atom, key_functions_atom],
        );
        Ok(bindings.bind_expr_no_span(&mut self.variables, expr))
    }

    fn merge_source_key_function(
        &mut self,
        merge_source: &ast::MergeSource,
    ) -> error::SpannedResult<(ir::AtomS, Bindings)> {
        let param_name = self.variables.new_name();
        let context_names = self.variables.push_context();
        let mut merged_key_bindings: Option<Bindings> = None;

        for merge_key in &merge_source.merge_keys {
            let key_bindings = if let Some(select) = &merge_key.select {
                self.expression(select)?
            } else if !merge_key.sequence_constructor.is_empty() {
                self.sequence_constructor_with_temporary_output_state(&merge_key.sequence_constructor)?
            } else {
                self.variables.context_item((0..0).into())?
            };
            let key_bindings = self.atomized_key_bindings(key_bindings, SortDataType::Text)?;
            merged_key_bindings = Some(match merged_key_bindings {
                Some(existing) => self.comma_bindings(existing, key_bindings),
                None => key_bindings,
            });
        }

        let return_bindings = merged_key_bindings.unwrap_or_else(|| {
            let empty_sequence = self.empty_sequence();
            Bindings::empty().bind_expr_no_span(&mut self.variables, empty_sequence.value)
        });
        self.variables.pop_context();

        let body = ir::Expr::Map(ir::Map {
            context_names,
            var_atom: Spanned::new(ir::Atom::Variable(param_name.clone()), (0..0).into()),
            return_expr: Box::new(return_bindings.expr()),
        });
        let function = ir::Expr::FunctionDefinition(ir::FunctionDefinition {
            params: vec![ir::Param {
                name: param_name,
                type_: None,
                default: None,
                required: false,
                original_name: None,
                tunnel: false,
            }],
            return_type: None,
            body: Box::new(Spanned::new(body, (0..0).into())),
        });

        let bindings = Bindings::empty().bind_expr_no_span(&mut self.variables, function);
        Ok(bindings.atom_bindings())
    }

    fn for_each_group(
        &mut self,
        for_each_group: &ast::ForEachGroup,
    ) -> error::SpannedResult<Bindings> {
        if for_each_group.group_starting_with.is_some()
            || for_each_group.group_ending_with.is_some()
            || for_each_group.composite
            || for_each_group.collation.is_some()
        {
            return Err(error::Error::Unsupported(format!(
                "Instruction not supported: {:?}",
                for_each_group
            ))
            .into());
        }

        // Determine the grouping mode and key expression
        let (group_key_expr, runtime_fn_name) = if let Some(group_by) = &for_each_group.group_by {
            (group_by, "xslt-for-each-group-by")
        } else if let Some(group_adjacent) = &for_each_group.group_adjacent {
            (group_adjacent, "xslt-for-each-group-adjacent")
        } else {
            return Err(error::Error::Unsupported(format!(
                "Instruction not supported: {:?}",
                for_each_group
            ))
            .into());
        };

        let (select_atom, bindings) = self.expression(&for_each_group.select)?.atom_bindings();
        let (key_function_atom, key_function_bindings) = self.group_key_function(group_key_expr)?;

        // Create a body closure with 3 params: context_item, position, last
        // This avoids using ir::Map (which would set position/last from the
        // single-item argument) and instead uses explicit Let bindings so the
        // runtime function can pass correct position/last values.
        let item_param = self.variables.new_name();
        let pos_param = self.variables.new_name();
        let last_param = self.variables.new_name();

        let context_names = self.variables.push_context();
        let body_bindings = self.with_template_continuation_availability(false, |this| {
            this.sequence_constructor(&for_each_group.sequence_constructor)
        })?;
        self.variables.pop_context();

        // Build Let chain: bind context variables from closure params
        let body_with_context = ir::Expr::Let(ir::Let {
            name: context_names.item.clone(),
            var_expr: Box::new(Spanned::new(
                ir::Expr::Atom(Spanned::new(
                    ir::Atom::Variable(item_param.clone()),
                    (0..0).into(),
                )),
                (0..0).into(),
            )),
            return_expr: Box::new(Spanned::new(
                ir::Expr::Let(ir::Let {
                    name: context_names.position.clone(),
                    var_expr: Box::new(Spanned::new(
                        ir::Expr::Atom(Spanned::new(
                            ir::Atom::Variable(pos_param.clone()),
                            (0..0).into(),
                        )),
                        (0..0).into(),
                    )),
                    return_expr: Box::new(Spanned::new(
                        ir::Expr::Let(ir::Let {
                            name: context_names.last.clone(),
                            var_expr: Box::new(Spanned::new(
                                ir::Expr::Atom(Spanned::new(
                                    ir::Atom::Variable(last_param.clone()),
                                    (0..0).into(),
                                )),
                                (0..0).into(),
                            )),
                            return_expr: Box::new(body_bindings.expr()),
                        }),
                        (0..0).into(),
                    )),
                }),
                (0..0).into(),
            )),
        });

        let body_closure_bindings =
            Bindings::empty().bind_expr_no_span(&mut self.variables, body_with_context);
        let (body_atom, body_fn_bindings) = self.closure(
            vec![
                ir::Param {
                    name: item_param,
                    type_: None,
                    default: None,
                    required: false,
                    original_name: None,
                    tunnel: false,
                },
                ir::Param {
                    name: pos_param,
                    type_: None,
                    default: None,
                    required: false,
                    original_name: None,
                    tunnel: false,
                },
                ir::Param {
                    name: last_param,
                    type_: None,
                    default: None,
                    required: false,
                    original_name: None,
                    tunnel: false,
                },
            ],
            body_closure_bindings,
        );

        // Compile sort specifications into a sort key function and options
        let sort_refs = for_each_group.sort.iter().collect::<Vec<_>>();
        let (sort_key_atom, sort_key_bindings, sort_descending, sort_numeric) =
            if !sort_refs.is_empty() {
                // Use first sort only for now; compile its key function
                let sort = sort_refs[0];
                let (key_atom, key_bindings) = self.sort_key_function(sort)?;
                let descending = self.sort_is_descending(sort)?;
                let numeric = matches!(self.sort_data_type(sort)?, SortDataType::Number);
                (key_atom, key_bindings, descending, numeric)
            } else {
                // No sort: pass empty sequence as sort key function
                let empty = Spanned::new(ir::Atom::Const(ir::Const::EmptySequence), (0..0).into());
                let bindings = Bindings::empty();
                (empty, bindings, false, false)
            };

        let sort_descending_atom = Spanned::new(
            ir::Atom::Const(ir::Const::String(
                if sort_descending { "yes" } else { "no" }.to_string(),
            )),
            (0..0).into(),
        );
        let sort_numeric_atom = Spanned::new(
            ir::Atom::Const(ir::Const::String(
                if sort_numeric { "yes" } else { "no" }.to_string(),
            )),
            (0..0).into(),
        );

        let expr = self.static_function_call_expr(
            runtime_fn_name,
            FN_NAMESPACE,
            6,
            vec![
                select_atom,
                key_function_atom,
                body_atom,
                sort_key_atom,
                sort_descending_atom,
                sort_numeric_atom,
            ],
        );

        Ok(bindings
            .concat(key_function_bindings)
            .concat(body_fn_bindings)
            .concat(sort_key_bindings)
            .bind_expr_no_span(&mut self.variables, expr))
    }

    fn group_key_function(
        &mut self,
        group_by: &ast::Expression,
    ) -> error::SpannedResult<(ir::AtomS, Bindings)> {
        let param_name = self.variables.new_name();
        let context_names = self.variables.push_context();
        let bindings = self.expression(group_by)?;
        let bindings = self.atomized_key_bindings(bindings, SortDataType::Text)?;
        self.variables.pop_context();

        let body = ir::Expr::Map(ir::Map {
            context_names,
            var_atom: Spanned::new(ir::Atom::Variable(param_name.clone()), (0..0).into()),
            return_expr: Box::new(bindings.expr()),
        });

        let function_definition = ir::FunctionDefinition {
            params: vec![ir::Param {
                name: param_name,
                type_: None,
                default: None,
                required: false,
                original_name: None,
                tunnel: false,
            }],
            return_type: None,
            body: Box::new(Spanned::new(body, (0..0).into())),
        };

        let function_expr = Bindings::empty().bind_expr_no_span(
            &mut self.variables,
            ir::Expr::FunctionDefinition(function_definition),
        );
        Ok(function_expr.atom_bindings())
    }

    fn iterate(&mut self, iterate: &ast::Iterate) -> error::SpannedResult<Bindings> {
        let (var_atom, bindings) = self.expression(&iterate.select)?.atom_bindings();

        let params = iterate
            .params
            .iter()
            .map(|param| -> error::SpannedResult<ir::IterateParam> {
                let param_bindings = self.select_or_sequence_constructor(param)?;
                let name = self.variables.declare_var_name(&param.name);
                Ok(ir::IterateParam {
                    name,
                    value: Box::new(param_bindings.expr()),
                    type_: param.as_.clone(),
                })
            })
            .collect::<error::SpannedResult<Vec<_>>>()?;

        let (context_names, loop_name) = self.variables.push_iterate_context();
        let return_bindings = self.with_template_continuation_availability(false, |this| {
            this.sequence_constructor(&iterate.sequence_constructor)
        })?;
        let on_complete_bindings = iterate
            .on_completion
            .as_ref()
            .map(|oc| {
                self.with_template_continuation_availability(false, |this| {
                    this.select_or_sequence_constructor(oc)
                })
            })
            .transpose()?;
        self.variables.pop_context();

        let expr = ir::Expr::Iterate(ir::Iterate {
            context_names,
            loop_name,
            var_atom,
            params,
            expr: Box::new(return_bindings.expr()),
            on_complete: on_complete_bindings.map(|x| Box::new(x.expr())),
        });

        Ok(bindings.bind_expr_no_span(&mut self.variables, expr))
    }

    fn break_(&mut self, break_: &ast::Break) -> error::SpannedResult<Bindings> {
        let loop_name = self
            .variables
            .current_iterate_loop_name()
            .ok_or(error::SpannedError {
                error: error::Error::XTSE3120,
                span: None,
            })?;

        let bindings = self.select_or_sequence_constructor(break_)?;
        let expr = ir::Expr::IterateBreak(ir::IterateBreak {
            loop_name,
            return_expr: Box::new(bindings.expr()),
        });
        Ok(bindings.bind_expr_no_span(&mut self.variables, expr))
    }

    fn next_iteration(
        &mut self,
        next_iteration: &ast::NextIteration,
    ) -> error::SpannedResult<Bindings> {
        let params = next_iteration
            .with_params
            .iter()
            .map(|param| {
                let value_bind = self.select_or_sequence_constructor(param)?;
                Ok(ir::IterateParam {
                    name: self.variables.new_var_name(&param.name),
                    value: Box::new(value_bind.expr()),
                    type_: param.as_.clone(),
                })
            })
            .collect::<error::SpannedResult<Vec<_>>>()?;

        let empty_sequence = self.empty_sequence();
        let return_expr = Bindings::new(
            self.variables
                .new_binding(empty_sequence.value, empty_sequence.span),
        );
        let let_next = ir::Expr::IterateLetNext(ir::IterateLetNext {
            params,
            return_expr: Box::new(return_expr.expr()),
        });
        let result = return_expr.bind_expr_no_span(&mut self.variables, let_next);
        Ok(result)
    }

    fn copy(&mut self, copy: &ast::Copy) -> error::SpannedResult<Bindings> {
        let (context_atom, bindings) = if let Some(select) = &copy.select {
            self.expression(select)?.atom_bindings()
        } else {
            self.variables.context_item((0..0).into())?.atom_bindings()
        };
        // copy shallow this item
        let expr = ir::Expr::CopyShallow(ir::CopyShallow {
            select: context_atom,
        });
        let (copy_atom, bindings) = bindings
            .bind_expr_no_span(&mut self.variables, expr)
            .atom_bindings();

        // if it is an element or document,
        // execute sequence constructor
        let is_element_expr = self.is_element_expr(copy_atom.clone());
        let (is_element_atom, bindings) = bindings
            .bind_expr_no_span(&mut self.variables, is_element_expr)
            .atom_bindings();
        let is_document_expr = self.is_document_expr(copy_atom.clone());
        let (is_document_atom, bindings) = bindings
            .bind_expr_no_span(&mut self.variables, is_document_expr)
            .atom_bindings();
        let is_element_or_document_expr = ir::Expr::Binary(ir::Binary {
            left: is_element_atom,
            op: ir::BinaryOperator::Or,
            right: is_document_atom,
        });
        let (is_element_or_document_atom, bindings) = bindings
            .bind_expr_no_span(&mut self.variables, is_element_or_document_expr)
            .atom_bindings();

        let copy_expr = ir::Expr::Atom(copy_atom.clone());

        let attribute_set_bindings =
            self.attribute_set_bindings(copy.use_attribute_sets.as_deref().unwrap_or(&[]))?;

        let sequence_constructor_bindings = if copy.select.is_some() {
            self.with_template_continuation_availability(false, |this| {
                this.sequence_constructor(&copy.sequence_constructor)
            })?
        } else {
            self.sequence_constructor(&copy.sequence_constructor)?
        };
        let append_content_bindings = if copy.use_attribute_sets.is_some() {
            self.comma_bindings(attribute_set_bindings, sequence_constructor_bindings)
        } else {
            sequence_constructor_bindings
        };
        let (append_content_atom, append_content_bindings) =
            append_content_bindings.atom_bindings();

        let bindings = bindings.concat(append_content_bindings);

        let append = ir::Expr::XmlAppend(ir::XmlAppend {
            parent: copy_atom,
            child: append_content_atom,
        });

        let if_expr = ir::Expr::If(ir::If {
            condition: is_element_or_document_atom,
            then: Box::new(Spanned::new(append, (0..0).into())),
            else_: Box::new(Spanned::new(copy_expr, (0..0).into())),
        });

        Ok(bindings.bind_expr_no_span(&mut self.variables, if_expr))
    }

    fn is_document_expr(&self, atom: ir::AtomS) -> ir::Expr {
        ir::Expr::InstanceOf(ir::InstanceOf {
            atom,
            sequence_type: xpath_ast::SequenceType::Item(xpath_ast::Item {
                item_type: xpath_ast::ItemType::KindTest(xpath_ast::KindTest::Document(None)),
                occurrence: xpath_ast::Occurrence::One,
            }),
        })
    }

    fn is_element_expr(&self, atom: ir::AtomS) -> ir::Expr {
        ir::Expr::InstanceOf(ir::InstanceOf {
            atom,
            sequence_type: xpath_ast::SequenceType::Item(xpath_ast::Item {
                item_type: xpath_ast::ItemType::KindTest(xpath_ast::KindTest::Element(None)),
                occurrence: xpath_ast::Occurrence::One,
            }),
        })
    }

    fn copy_of(&mut self, copy_of: &ast::CopyOf) -> error::SpannedResult<Bindings> {
        let (atom, bindings) = self.expression(&copy_of.select)?.atom_bindings();
        let copy_deep_expr = ir::Expr::CopyDeep(ir::CopyDeep { select: atom });
        Ok(bindings.bind_expr_no_span(&mut self.variables, copy_deep_expr))
    }

    fn sequence(&mut self, sequence: &ast::Sequence) -> error::SpannedResult<Bindings> {
        self.select_or_sequence_constructor(sequence)
    }

    fn xml_name(&mut self, name: &ast::Name) -> error::SpannedResult<Bindings> {
        let local_name = Spanned::new(
            ir::Atom::Const(ir::Const::String(name.local_name().to_string())),
            (0..0).into(),
        );
        let namespace = Spanned::new(
            ir::Atom::Const(ir::Const::String(name.namespace().to_string())),
            (0..0).into(),
        );

        let binding = self
            .variables
            .new_binding_no_span(ir::Expr::XmlName(ir::XmlName {
                local_name,
                namespace,
            }));
        Ok(Bindings::new(binding))
    }

    fn xml_name_dynamic(
        &mut self,
        name: &ast::ValueTemplate<String>,
        namespace: &Option<ast::ValueTemplate<String>>,
        namespaces: &[ast::LiteralNamespace],
        default_namespace: &str,
    ) -> error::SpannedResult<Bindings> {
        let literal_name = self.static_value_template(name);
        let (localname_atom, bindings) = if let Some((local_name, namespace_uri)) =
            literal_name.as_deref().and_then(|literal_name| {
                self.resolve_static_qname_with_default(literal_name, namespaces, default_namespace)
            }) {
            let local_name_atom = Spanned::new(
                ir::Atom::Const(ir::Const::String(local_name)),
                (0..0).into(),
            );
            let bindings = Bindings::empty()
                .bind_expr_no_span(&mut self.variables, ir::Expr::Atom(local_name_atom));
            if namespace.is_none() {
                let namespace_atom = Spanned::new(
                    ir::Atom::Const(ir::Const::String(namespace_uri)),
                    (0..0).into(),
                );
                let namespace_bindings = Bindings::empty()
                    .bind_expr_no_span(&mut self.variables, ir::Expr::Atom(namespace_atom));
                let (local_name_atom, bindings) = bindings.atom_bindings();
                let (namespace_atom, namespace_bindings) = namespace_bindings.atom_bindings();
                let name = ir::Expr::XmlName(ir::XmlName {
                    local_name: local_name_atom,
                    namespace: namespace_atom,
                });
                return Ok(bindings
                    .concat(namespace_bindings)
                    .bind_expr_no_span(&mut self.variables, name));
            }
            bindings.atom_bindings()
        } else if namespace.is_none() || literal_name.is_none() {
            let (lexical_name_atom, bindings) =
                self.attribute_value_template(name)?.atom_bindings();
            let default_namespace_atom = Spanned::new(
                ir::Atom::Const(ir::Const::String(default_namespace.to_string())),
                (0..0).into(),
            );
            let namespace_map_atom = Spanned::new(
                ir::Atom::Const(ir::Const::String(
                    self.encode_literal_namespaces(namespaces),
                )),
                (0..0).into(),
            );
            let (force_namespace_atom, namespace_bindings) = if let Some(namespace) = namespace {
                self.attribute_value_template(namespace)?.atom_bindings()
            } else {
                (
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String(String::new())),
                        (0..0).into(),
                    ),
                    Bindings::empty(),
                )
            };
            let expr = self.static_function_call_expr(
                "resolve-xslt-qname",
                FN_NAMESPACE,
                4,
                vec![
                    lexical_name_atom,
                    default_namespace_atom,
                    namespace_map_atom,
                    force_namespace_atom,
                ],
            );
            return Ok(bindings
                .concat(namespace_bindings)
                .bind_expr_no_span(&mut self.variables, expr));
        } else {
            self.attribute_value_template(name)?.atom_bindings()
        };
        let (namespace_atom, namespace_bindings) = if let Some(namespace) = namespace {
            self.attribute_value_template(namespace)?.atom_bindings()
        } else {
            let namespace_atom = self.empty_string();
            (namespace_atom, Bindings::empty())
        };
        let bindings = bindings.concat(namespace_bindings);
        let name = ir::Expr::XmlName(ir::XmlName {
            local_name: localname_atom,
            namespace: namespace_atom,
        });
        Ok(bindings.bind_expr_no_span(&mut self.variables, name))
    }

    fn ncname_dynamic(
        &mut self,
        name: &ast::ValueTemplate<String>,
    ) -> error::SpannedResult<Bindings> {
        self.attribute_value_template(name)
    }

    fn static_value_template<V>(&self, value_template: &ast::ValueTemplate<V>) -> Option<String>
    where
        V: Clone + PartialEq + Eq,
    {
        let mut value = String::new();
        for item in &value_template.template {
            match item {
                ast::ValueTemplateItem::String { text, .. } => value.push_str(text),
                ast::ValueTemplateItem::Curly { c } => value.push(*c),
                ast::ValueTemplateItem::Value { .. } => return None,
            }
        }
        Some(value)
    }

    fn resolve_static_qname_with_default(
        &self,
        lexical_qname: &str,
        namespaces: &[ast::LiteralNamespace],
        default_namespace: &str,
    ) -> Option<(String, String)> {
        let Some((prefix, local_name)) = lexical_qname.split_once(':') else {
            return Some((lexical_qname.to_string(), default_namespace.to_string()));
        };
        let namespace = namespaces
            .iter()
            .find(|namespace| namespace.prefix == prefix)
            .map(|namespace| namespace.uri.as_str())
            .or_else(|| self.current_static_context().namespaces().by_prefix(prefix))?;
        Some((local_name.to_string(), namespace.to_string()))
    }

    fn default_element_namespace(&self, namespaces: &[ast::LiteralNamespace]) -> String {
        namespaces
            .iter()
            .find(|namespace| namespace.prefix.is_empty())
            .map(|namespace| namespace.uri.clone())
            .unwrap_or_else(|| {
                self.current_static_context()
                    .namespaces()
                    .default_element_namespace()
                    .to_string()
            })
    }

    fn encode_literal_namespaces(&self, namespaces: &[ast::LiteralNamespace]) -> String {
        let mut encoded = Vec::new();
        let mut seen = HashSet::new();
        for namespace in namespaces {
            let key = (namespace.prefix.clone(), namespace.uri.clone());
            if seen.insert(key.clone()) {
                encoded.push(format!(
                    "{}={}",
                    Self::hex_encode(&key.0),
                    Self::hex_encode(&key.1)
                ));
            }
        }
        encoded.join("|")
    }

    fn hex_encode(value: &str) -> String {
        value
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect()
    }

    fn static_name_namespace(
        &self,
        name: &ast::ValueTemplate<String>,
        namespace: &Option<ast::ValueTemplate<String>>,
        namespaces: &[ast::LiteralNamespace],
        default_namespace: &str,
    ) -> Option<ast::LiteralNamespace> {
        let literal_name = self.static_value_template(name)?;
        let (prefix, uri) = if let Some(namespace) = namespace {
            let prefix = literal_name
                .split_once(':')
                .map(|(prefix, _)| prefix.to_string())
                .unwrap_or_default();
            (prefix, self.static_value_template(namespace)?)
        } else if let Some((prefix, _)) = literal_name.split_once(':') {
            let uri = namespaces
                .iter()
                .find(|namespace| namespace.prefix == prefix)
                .map(|namespace| namespace.uri.clone())
                .or_else(|| {
                    self.current_static_context()
                        .namespaces()
                        .by_prefix(prefix)
                        .map(str::to_string)
                })?;
            (prefix.to_string(), uri)
        } else if default_namespace.is_empty() {
            return None;
        } else {
            (String::new(), default_namespace.to_string())
        };
        Some(ast::LiteralNamespace { prefix, uri })
    }

    fn element(&mut self, element: &ast::Element) -> error::SpannedResult<Bindings> {
        let default_namespace = self.default_element_namespace(&element.namespaces);
        let (name_atom, bindings) = self
            .xml_name_dynamic(
                &element.name,
                &element.namespace,
                &element.namespaces,
                &default_namespace,
            )?
            .atom_bindings();

        let expr = ir::Expr::XmlElement(ir::XmlElement { name: name_atom });
        let (element_atom, bindings) = bindings
            .bind_expr_no_span(&mut self.variables, expr)
            .atom_bindings();
        let (element_atom, bindings) = if let Some(namespace) = self.static_name_namespace(
            &element.name,
            &element.namespace,
            &element.namespaces,
            &default_namespace,
        ) {
            let prefix_atom = Spanned::new(
                ir::Atom::Const(ir::Const::String(namespace.prefix)),
                (0..0).into(),
            );
            let namespace_atom = Spanned::new(
                ir::Atom::Const(ir::Const::String(namespace.uri)),
                (0..0).into(),
            );
            let namespace_expr = ir::Expr::XmlNamespace(ir::XmlNamespace {
                prefix: prefix_atom,
                namespace: namespace_atom,
            });
            let (namespace_atom, namespace_bindings) = Bindings::empty()
                .bind_expr_no_span(&mut self.variables, namespace_expr)
                .atom_bindings();
            bindings
                .concat(namespace_bindings)
                .bind_expr_no_span(
                    &mut self.variables,
                    ir::Expr::XmlAppend(ir::XmlAppend {
                        parent: element_atom,
                        child: namespace_atom,
                    }),
                )
                .atom_bindings()
        } else {
            (element_atom, bindings)
        };
        let attribute_set_bindings = self.attribute_set_append(
            element_atom.clone(),
            element.use_attribute_sets.as_deref().unwrap_or(&[]),
        )?;
        let sequence_constructor_bindings =
            self.sequence_constructor_append(element_atom, &element.sequence_constructor)?;
        Ok(bindings
            .concat(attribute_set_bindings)
            .concat(sequence_constructor_bindings))
    }

    fn document(&mut self, document: &ast::Document) -> error::SpannedResult<Bindings> {
        if document.validation.is_some() || document.type_.is_some() {
            return Err(error::Error::Unsupported(format!(
                "Instruction not supported: {:?}",
                document
            ))
            .into());
        }

        let expr = ir::Expr::XmlDocument(ir::XmlRoot {});
        let (document_atom, bindings) = Bindings::empty()
            .bind_expr_no_span(&mut self.variables, expr)
            .atom_bindings();
        let sequence_constructor_bindings =
            self.sequence_constructor_append(document_atom, &document.sequence_constructor)?;
        Ok(bindings.concat(sequence_constructor_bindings))
    }

    fn source_document(
        &mut self,
        source_document: &ast::SourceDocument,
    ) -> error::SpannedResult<Bindings> {
        if source_document.streamable
            || source_document.use_accumulators.is_some()
            || source_document.validation.is_some()
            || source_document.type_.is_some()
        {
            return Err(error::Error::Unsupported(format!(
                "Instruction not supported: {:?}",
                source_document
            ))
            .into());
        }

        let (href_atom, href_bindings) = self
            .attribute_value_template(&source_document.href)?
            .atom_bindings();
        let load_expr = self.static_function_call_expr("doc", FN_NAMESPACE, 1, vec![href_atom]);
        let load_bindings = href_bindings.bind_expr(
            &mut self.variables,
            Spanned::new(
                load_expr,
                (source_document.span.start..source_document.span.end).into(),
            ),
        );
        let (document_atom, load_bindings) = load_bindings.atom_bindings();
        let (document_atom, bindings) = if self.strip_source_document_whitespace {
            let strip_expr = self.static_function_call_expr(
                "strip-space-document",
                FN_NAMESPACE,
                1,
                vec![document_atom],
            );
            load_bindings
                .bind_expr_no_span(&mut self.variables, strip_expr)
                .atom_bindings()
        } else {
            (document_atom, load_bindings)
        };

        let context_names = self.variables.push_context();
        let return_bindings = self.sequence_constructor(&source_document.sequence_constructor)?;
        self.variables.pop_context();
        let expr = ir::Expr::Map(ir::Map {
            context_names,
            var_atom: document_atom,
            return_expr: Box::new(return_bindings.expr()),
        });

        Ok(bindings.bind_expr(
            &mut self.variables,
            Spanned::new(
                expr,
                (source_document.span.start..source_document.span.end).into(),
            ),
        ))
    }

    fn text(&mut self, text: &ast::Text) -> error::SpannedResult<Bindings> {
        let (atom, bindings) = self
            .attribute_value_template(&text.content)?
            .atom_bindings();
        Ok(bindings.bind_expr_no_span(
            &mut self.variables,
            ir::Expr::XmlText(ir::XmlText { value: atom }),
        ))
    }

    fn attribute(&mut self, attribute: &ast::Attribute) -> error::SpannedResult<Bindings> {
        let (name_atom, name_bindings) = self
            .xml_name_dynamic(
                &attribute.name,
                &attribute.namespace,
                &attribute.namespaces,
                "",
            )?
            .atom_bindings();
        let value_bindings = if self.processor_xslt_version() < 3
            && attribute.select.is_none()
            && !attribute.sequence_constructor.is_empty()
        {
            self.select_or_sequence_constructor_with_temporary_output_state(attribute)?
        } else {
            self.select_or_sequence_constructor(attribute)?
        };
        let (value_atom, value_bindings) = value_bindings.atom_bindings();
        let (separator_atom, separator_bindings) = if let Some(separator) = &attribute.separator {
            self.attribute_value_template(separator)?.atom_bindings()
        } else if attribute.select.is_some() {
            (self.space_separator_atom(), Bindings::empty())
        } else {
            (self.empty_string(), Bindings::empty())
        };
        let simple_content_expr = self.simple_content_expr(value_atom, separator_atom);
        let text_bindings = value_bindings
            .concat(separator_bindings)
            .bind_expr_no_span(&mut self.variables, simple_content_expr);
        let (text_atom, text_bindings) = text_bindings.atom_bindings();
        let bindings = name_bindings.concat(text_bindings);
        Ok(bindings.bind_expr_no_span(
            &mut self.variables,
            ir::Expr::XmlAttribute(ir::XmlAttribute {
                name: name_atom,
                value: text_atom,
            }),
        ))
    }

    fn namespace(&mut self, namespace: &ast::Namespace) -> error::SpannedResult<Bindings> {
        let (ncname_atom, ncname_bindings) = self.ncname_dynamic(&namespace.name)?.atom_bindings();
        let text_bindings = if self.processor_xslt_version() < 3
            && namespace.select.is_none()
            && !namespace.sequence_constructor.is_empty()
        {
            let content_bindings =
                self.select_or_sequence_constructor_with_temporary_output_state(namespace)?;
            self.simple_content_bindings(content_bindings, self.space_separator_atom())
        } else {
            self.select_or_sequence_constructor_simple_content(namespace)?
        };
        let (text_atom, text_bindings) = text_bindings.atom_bindings();
        let bindings = ncname_bindings.concat(text_bindings);
        Ok(bindings.bind_expr_no_span(
            &mut self.variables,
            ir::Expr::XmlNamespace(ir::XmlNamespace {
                prefix: ncname_atom,
                namespace: text_atom,
            }),
        ))
    }

    fn comment(&mut self, comment: &ast::Comment) -> error::SpannedResult<Bindings> {
        let bindings = if self.processor_xslt_version() < 3
            && comment.select.is_none()
            && !comment.sequence_constructor.is_empty()
        {
            let content_bindings =
                self.select_or_sequence_constructor_with_temporary_output_state(comment)?;
            self.simple_content_bindings(content_bindings, self.space_separator_atom())
        } else {
            self.select_or_sequence_constructor_simple_content(comment)?
        };
        let (atom, bindings) = bindings.atom_bindings();
        Ok(bindings.bind_expr_no_span(
            &mut self.variables,
            ir::Expr::XmlComment(ir::XmlComment { value: atom }),
        ))
    }

    fn processing_instruction(
        &mut self,
        pi: &ast::ProcessingInstruction,
    ) -> error::SpannedResult<Bindings> {
        let (ncname_atom, ncname_bindings) = self.ncname_dynamic(&pi.name)?.atom_bindings();
        let content_bindings = if self.processor_xslt_version() < 3
            && pi.select.is_none()
            && !pi.sequence_constructor.is_empty()
        {
            let content_bindings =
                self.select_or_sequence_constructor_with_temporary_output_state(pi)?;
            self.simple_content_bindings(content_bindings, self.space_separator_atom())
        } else {
            self.select_or_sequence_constructor_simple_content(pi)?
        };
        let (content_atom, content_bindings) = content_bindings.atom_bindings();
        let bindings = ncname_bindings.concat(content_bindings);
        Ok(bindings.bind_expr_no_span(
            &mut self.variables,
            ir::Expr::XmlProcessingInstruction(ir::XmlProcessingInstruction {
                target: ncname_atom,
                content: content_atom,
            }),
        ))
    }

    // fn throw_error(&mut self) -> error::SpannedResult<Bindings> {
    //     let error_atom = self.error_atom();
    //     let expr = ir::Expr::FunctionCall(ir::FunctionCall {
    //         atom: Spanned::new(error_atom, (0..0).into()),
    //         args: vec![],
    //     });
    //     Ok(Bindings::new(self.variables.new_binding_no_span(expr)))
    // }

    fn expression(&mut self, expression: &ast::Expression) -> error::SpannedResult<Bindings> {
        let mut rewritten_xpath = expression.xpath.0.clone();
        self.offset_xpath_spans_expr(&mut rewritten_xpath, expression.span.start);
        let current_focus = self.bind_current_focus_variable(&mut rewritten_xpath);
        self.rewrite_user_function_references_expr(&mut rewritten_xpath, &expression.namespaces);
        let static_context = self.current_static_context().clone_with_static_base_uri(
            self.current_static_context()
                .static_base_uri()
                .map(ToOwned::to_owned),
        );
        let mut ir_converter =
            xee_xpath_compiler::IrConverter::new(&mut self.variables, &static_context);
        let bindings = ir_converter.expr(&rewritten_xpath);
        let current_focus = current_focus?;
        if let Some((_, current_name)) = &current_focus {
            self.variables
                .remove_var_name_in_current_scope(current_name);
        }
        let bindings = bindings?;
        Ok(match current_focus {
            Some((current_bindings, _)) => current_bindings.concat(bindings),
            None => bindings,
        })
    }

    fn bind_current_focus_variable(
        &mut self,
        expr: &mut xpath_ast::ExprS,
    ) -> error::SpannedResult<Option<(Bindings, Name)>> {
        let current_name = Name::new(
            "__xee_current_focus".to_string(),
            "urn:xee:internal".to_string(),
            String::new(),
        );
        if !self.rewrite_current_focus_expr(expr, &current_name) {
            return Ok(None);
        }

        let Some(context_names) = self.variables.current_context_names() else {
            return Ok(None);
        };

        let ir_name = self.variables.new_name();
        let binding = xee_ir::Binding::new(
            ir_name.clone(),
            ir::Expr::Atom(Spanned::new(
                ir::Atom::Variable(context_names.item),
                (0..0).into(),
            )),
            (0..0).into(),
        );
        self.variables
            .insert_var_name_in_current_scope(current_name.clone(), ir_name);
        Ok(Some((Bindings::new(binding), current_name)))
    }

    fn rewrite_current_focus_expr(&self, expr: &mut xpath_ast::ExprS, current_name: &Name) -> bool {
        let mut rewritten = false;
        for expr_single in &mut expr.value.0 {
            rewritten |= self.rewrite_current_focus_expr_single(expr_single, current_name);
        }
        rewritten
    }

    fn rewrite_current_focus_expr_or_empty(
        &self,
        expr: &mut xpath_ast::ExprOrEmptyS,
        current_name: &Name,
    ) -> bool {
        let Some(expr_value) = &mut expr.value else {
            return false;
        };
        let mut rewritten = false;
        for expr_single in &mut expr_value.0 {
            rewritten |= self.rewrite_current_focus_expr_single(expr_single, current_name);
        }
        rewritten
    }

    fn rewrite_current_focus_expr_single(
        &self,
        expr: &mut xpath_ast::ExprSingleS,
        current_name: &Name,
    ) -> bool {
        match &mut expr.value {
            xpath_ast::ExprSingle::Path(path_expr) => {
                self.rewrite_current_focus_path_expr(path_expr, current_name)
            }
            xpath_ast::ExprSingle::Apply(apply_expr) => {
                let mut rewritten =
                    self.rewrite_current_focus_path_expr(&mut apply_expr.path_expr, current_name);
                if let xpath_ast::ApplyOperator::SimpleMap(path_exprs) = &mut apply_expr.operator {
                    for path_expr in path_exprs {
                        rewritten |= self.rewrite_current_focus_path_expr(path_expr, current_name);
                    }
                }
                rewritten
            }
            xpath_ast::ExprSingle::Let(let_expr) => {
                self.rewrite_current_focus_expr_single(&mut let_expr.var_expr, current_name)
                    | self
                        .rewrite_current_focus_expr_single(&mut let_expr.return_expr, current_name)
            }
            xpath_ast::ExprSingle::If(if_expr) => {
                self.rewrite_current_focus_expr(&mut if_expr.condition, current_name)
                    | self.rewrite_current_focus_expr_single(&mut if_expr.then, current_name)
                    | self.rewrite_current_focus_expr_single(&mut if_expr.else_, current_name)
            }
            xpath_ast::ExprSingle::Binary(binary_expr) => {
                self.rewrite_current_focus_path_expr(&mut binary_expr.left, current_name)
                    | self.rewrite_current_focus_path_expr(&mut binary_expr.right, current_name)
            }
            xpath_ast::ExprSingle::For(for_expr) => {
                self.rewrite_current_focus_expr_single(&mut for_expr.var_expr, current_name)
                    | self
                        .rewrite_current_focus_expr_single(&mut for_expr.return_expr, current_name)
            }
            xpath_ast::ExprSingle::Quantified(quantified_expr) => {
                self.rewrite_current_focus_expr_single(&mut quantified_expr.var_expr, current_name)
                    | self.rewrite_current_focus_expr_single(
                        &mut quantified_expr.satisfies_expr,
                        current_name,
                    )
            }
        }
    }

    fn rewrite_current_focus_path_expr(
        &self,
        path_expr: &mut xpath_ast::PathExpr,
        current_name: &Name,
    ) -> bool {
        let mut rewritten = false;
        for step in &mut path_expr.steps {
            rewritten |= self.rewrite_current_focus_step_expr(step, current_name);
        }
        rewritten
    }

    fn rewrite_current_focus_step_expr(
        &self,
        step: &mut xpath_ast::StepExprS,
        current_name: &Name,
    ) -> bool {
        match &mut step.value {
            xpath_ast::StepExpr::PrimaryExpr(primary) => {
                self.rewrite_current_focus_primary_expr(primary, current_name)
            }
            xpath_ast::StepExpr::PostfixExpr { primary, postfixes } => {
                let mut rewritten = self.rewrite_current_focus_primary_expr(primary, current_name);
                for postfix in postfixes {
                    match postfix {
                        xpath_ast::Postfix::Predicate(expr) => {
                            rewritten |= self.rewrite_current_focus_expr(expr, current_name);
                        }
                        xpath_ast::Postfix::ArgumentList(arguments) => {
                            for argument in arguments {
                                rewritten |=
                                    self.rewrite_current_focus_expr_single(argument, current_name);
                            }
                        }
                        xpath_ast::Postfix::Lookup(key_specifier) => {
                            rewritten |= self
                                .rewrite_current_focus_key_specifier(key_specifier, current_name);
                        }
                    }
                }
                rewritten
            }
            xpath_ast::StepExpr::AxisStep(axis_step) => {
                let mut rewritten = false;
                for predicate in &mut axis_step.predicates {
                    rewritten |= self.rewrite_current_focus_expr(predicate, current_name);
                }
                rewritten
            }
        }
    }

    fn rewrite_current_focus_primary_expr(
        &self,
        primary: &mut xpath_ast::PrimaryExprS,
        current_name: &Name,
    ) -> bool {
        match &mut primary.value {
            xpath_ast::PrimaryExpr::FunctionCall(function_call)
                if function_call.name.value.namespace() == FN_NAMESPACE
                    && function_call.name.value.local_name() == "current"
                    && function_call.arguments.is_empty() =>
            {
                primary.value = xpath_ast::PrimaryExpr::VarRef(current_name.clone());
                true
            }
            xpath_ast::PrimaryExpr::FunctionCall(function_call) => {
                let mut rewritten = false;
                for argument in &mut function_call.arguments {
                    rewritten |= self.rewrite_current_focus_expr_single(argument, current_name);
                }
                rewritten
            }
            xpath_ast::PrimaryExpr::Expr(expr) => {
                self.rewrite_current_focus_expr_or_empty(expr, current_name)
            }
            xpath_ast::PrimaryExpr::InlineFunction(inline_function) => {
                self.rewrite_current_focus_expr_or_empty(&mut inline_function.body, current_name)
            }
            xpath_ast::PrimaryExpr::MapConstructor(map_constructor) => {
                let mut rewritten = false;
                for entry in &mut map_constructor.entries {
                    rewritten |=
                        self.rewrite_current_focus_expr_single(&mut entry.key, current_name);
                    rewritten |=
                        self.rewrite_current_focus_expr_single(&mut entry.value, current_name);
                }
                rewritten
            }
            xpath_ast::PrimaryExpr::ArrayConstructor(array_constructor) => {
                match array_constructor {
                    xpath_ast::ArrayConstructor::Square(expr) => {
                        self.rewrite_current_focus_expr(expr, current_name)
                    }
                    xpath_ast::ArrayConstructor::Curly(expr) => {
                        self.rewrite_current_focus_expr_or_empty(expr, current_name)
                    }
                }
            }
            xpath_ast::PrimaryExpr::UnaryLookup(key_specifier) => {
                self.rewrite_current_focus_key_specifier(key_specifier, current_name)
            }
            xpath_ast::PrimaryExpr::Literal(_)
            | xpath_ast::PrimaryExpr::VarRef(_)
            | xpath_ast::PrimaryExpr::ContextItem
            | xpath_ast::PrimaryExpr::NamedFunctionRef(_) => false,
        }
    }

    fn rewrite_current_focus_key_specifier(
        &self,
        key_specifier: &mut xpath_ast::KeySpecifier,
        current_name: &Name,
    ) -> bool {
        match key_specifier {
            xpath_ast::KeySpecifier::Expr(expr) => {
                self.rewrite_current_focus_expr_or_empty(expr, current_name)
            }
            xpath_ast::KeySpecifier::NcName(_)
            | xpath_ast::KeySpecifier::Integer(_)
            | xpath_ast::KeySpecifier::Star => false,
        }
    }

    fn xpath(
        &mut self,
        xpath: &xee_xpath_ast::ast::ExprS,
        namespaces: &[ast::LiteralNamespace],
    ) -> error::SpannedResult<Bindings> {
        let mut rewritten_xpath = xpath.clone();
        self.rewrite_user_function_references_expr(&mut rewritten_xpath, namespaces);
        let static_context = self.current_static_context().clone_with_static_base_uri(
            self.current_static_context()
                .static_base_uri()
                .map(ToOwned::to_owned),
        );
        let mut ir_converter =
            xee_xpath_compiler::IrConverter::new(&mut self.variables, &static_context);
        ir_converter.expr(&rewritten_xpath)
    }

    fn offset_xpath_spans_expr(&self, expr: &mut xpath_ast::ExprS, offset: usize) {
        expr.span = Self::offset_xpath_span(expr.span, offset);
        for expr_single in &mut expr.value.0 {
            self.offset_xpath_spans_expr_single(expr_single, offset);
        }
    }

    fn offset_xpath_spans_expr_or_empty(&self, expr: &mut xpath_ast::ExprOrEmptyS, offset: usize) {
        expr.span = Self::offset_xpath_span(expr.span, offset);
        if let Some(expr_value) = &mut expr.value {
            expr_value.0.iter_mut().for_each(|expr_single| {
                self.offset_xpath_spans_expr_single(expr_single, offset);
            });
        }
    }

    fn offset_xpath_spans_expr_single(&self, expr: &mut xpath_ast::ExprSingleS, offset: usize) {
        expr.span = Self::offset_xpath_span(expr.span, offset);
        match &mut expr.value {
            xpath_ast::ExprSingle::Path(path_expr) => {
                self.offset_xpath_spans_path_expr(path_expr, offset);
            }
            xpath_ast::ExprSingle::Apply(apply_expr) => {
                self.offset_xpath_spans_path_expr(&mut apply_expr.path_expr, offset);
                if let xpath_ast::ApplyOperator::SimpleMap(path_exprs) = &mut apply_expr.operator {
                    for path_expr in path_exprs {
                        self.offset_xpath_spans_path_expr(path_expr, offset);
                    }
                }
            }
            xpath_ast::ExprSingle::Let(let_expr) => {
                self.offset_xpath_spans_expr_single(&mut let_expr.var_expr, offset);
                self.offset_xpath_spans_expr_single(&mut let_expr.return_expr, offset);
            }
            xpath_ast::ExprSingle::If(if_expr) => {
                self.offset_xpath_spans_expr(&mut if_expr.condition, offset);
                self.offset_xpath_spans_expr_single(&mut if_expr.then, offset);
                self.offset_xpath_spans_expr_single(&mut if_expr.else_, offset);
            }
            xpath_ast::ExprSingle::Binary(binary_expr) => {
                self.offset_xpath_spans_path_expr(&mut binary_expr.left, offset);
                self.offset_xpath_spans_path_expr(&mut binary_expr.right, offset);
            }
            xpath_ast::ExprSingle::For(for_expr) => {
                self.offset_xpath_spans_expr_single(&mut for_expr.var_expr, offset);
                self.offset_xpath_spans_expr_single(&mut for_expr.return_expr, offset);
            }
            xpath_ast::ExprSingle::Quantified(quantified_expr) => {
                self.offset_xpath_spans_expr_single(&mut quantified_expr.var_expr, offset);
                self.offset_xpath_spans_expr_single(&mut quantified_expr.satisfies_expr, offset);
            }
        }
    }

    fn offset_xpath_spans_path_expr(&self, path_expr: &mut xpath_ast::PathExpr, offset: usize) {
        for step in &mut path_expr.steps {
            self.offset_xpath_spans_step_expr(step, offset);
        }
    }

    fn offset_xpath_spans_step_expr(&self, step: &mut xpath_ast::StepExprS, offset: usize) {
        step.span = Self::offset_xpath_span(step.span, offset);
        match &mut step.value {
            xpath_ast::StepExpr::PrimaryExpr(primary) => {
                self.offset_xpath_spans_primary_expr(primary, offset);
            }
            xpath_ast::StepExpr::PostfixExpr { primary, postfixes } => {
                self.offset_xpath_spans_primary_expr(primary, offset);
                for postfix in postfixes {
                    self.offset_xpath_spans_postfix(postfix, offset);
                }
            }
            xpath_ast::StepExpr::AxisStep(axis_step) => {
                for predicate in &mut axis_step.predicates {
                    self.offset_xpath_spans_expr(predicate, offset);
                }
            }
        }
    }

    fn offset_xpath_spans_primary_expr(
        &self,
        primary: &mut xpath_ast::PrimaryExprS,
        offset: usize,
    ) {
        primary.span = Self::offset_xpath_span(primary.span, offset);
        match &mut primary.value {
            xpath_ast::PrimaryExpr::FunctionCall(function_call) => {
                function_call.name.span = Self::offset_xpath_span(function_call.name.span, offset);
                for argument in &mut function_call.arguments {
                    self.offset_xpath_spans_expr_single(argument, offset);
                }
            }
            xpath_ast::PrimaryExpr::NamedFunctionRef(named_function_ref) => {
                named_function_ref.name.span =
                    Self::offset_xpath_span(named_function_ref.name.span, offset);
            }
            xpath_ast::PrimaryExpr::Expr(expr) => {
                self.offset_xpath_spans_expr_or_empty(expr, offset);
            }
            xpath_ast::PrimaryExpr::InlineFunction(inline_function) => {
                self.offset_xpath_spans_expr_or_empty(&mut inline_function.body, offset);
            }
            xpath_ast::PrimaryExpr::MapConstructor(map_constructor) => {
                for entry in &mut map_constructor.entries {
                    self.offset_xpath_spans_expr_single(&mut entry.key, offset);
                    self.offset_xpath_spans_expr_single(&mut entry.value, offset);
                }
            }
            xpath_ast::PrimaryExpr::ArrayConstructor(array_constructor) => {
                match array_constructor {
                    xpath_ast::ArrayConstructor::Square(expr) => {
                        self.offset_xpath_spans_expr(expr, offset);
                    }
                    xpath_ast::ArrayConstructor::Curly(expr) => {
                        self.offset_xpath_spans_expr_or_empty(expr, offset);
                    }
                }
            }
            xpath_ast::PrimaryExpr::UnaryLookup(key_specifier) => {
                self.offset_xpath_spans_key_specifier(key_specifier, offset);
            }
            xpath_ast::PrimaryExpr::Literal(_)
            | xpath_ast::PrimaryExpr::VarRef(_)
            | xpath_ast::PrimaryExpr::ContextItem => {}
        }
    }

    fn offset_xpath_spans_postfix(&self, postfix: &mut xpath_ast::Postfix, offset: usize) {
        match postfix {
            xpath_ast::Postfix::Predicate(expr) => self.offset_xpath_spans_expr(expr, offset),
            xpath_ast::Postfix::ArgumentList(arguments) => {
                for argument in arguments {
                    self.offset_xpath_spans_expr_single(argument, offset);
                }
            }
            xpath_ast::Postfix::Lookup(key_specifier) => {
                self.offset_xpath_spans_key_specifier(key_specifier, offset);
            }
        }
    }

    fn offset_xpath_spans_key_specifier(
        &self,
        key_specifier: &mut xpath_ast::KeySpecifier,
        offset: usize,
    ) {
        if let xpath_ast::KeySpecifier::Expr(expr) = key_specifier {
            self.offset_xpath_spans_expr_or_empty(expr, offset);
        }
    }

    fn offset_xpath_span(span: xpath_ast::Span, offset: usize) -> xpath_ast::Span {
        xpath_ast::Span::new(span.start + offset, span.end + offset)
    }

    fn lookup_xslt_function_var_name(&self, name: &OwnedName, arity: u8) -> Option<OwnedName> {
        self.xslt_functions.get(&(name.clone(), arity)).cloned()
    }

    fn rewrite_user_function_references_expr(
        &mut self,
        expr: &mut xpath_ast::ExprS,
        namespaces: &[ast::LiteralNamespace],
    ) {
        for expr_single in &mut expr.value.0 {
            self.rewrite_user_function_references_expr_single(expr_single, namespaces);
        }
    }

    fn rewrite_user_function_references_expr_or_empty(
        &mut self,
        expr: &mut xpath_ast::ExprOrEmptyS,
        namespaces: &[ast::LiteralNamespace],
    ) {
        if let Some(expr) = &mut expr.value {
            for expr_single in &mut expr.0 {
                self.rewrite_user_function_references_expr_single(expr_single, namespaces);
            }
        }
    }

    fn rewrite_user_function_references_expr_single(
        &mut self,
        expr: &mut xpath_ast::ExprSingleS,
        namespaces: &[ast::LiteralNamespace],
    ) {
        match &mut expr.value {
            xpath_ast::ExprSingle::Path(path_expr) => {
                self.rewrite_user_function_references_path_expr(path_expr, namespaces);
            }
            xpath_ast::ExprSingle::Apply(apply_expr) => {
                self.rewrite_user_function_references_path_expr(
                    &mut apply_expr.path_expr,
                    namespaces,
                );
                if let xpath_ast::ApplyOperator::SimpleMap(path_exprs) = &mut apply_expr.operator {
                    for path_expr in path_exprs {
                        self.rewrite_user_function_references_path_expr(path_expr, namespaces);
                    }
                }
            }
            xpath_ast::ExprSingle::Let(let_expr) => {
                self.rewrite_user_function_references_expr_single(
                    &mut let_expr.var_expr,
                    namespaces,
                );
                self.rewrite_user_function_references_expr_single(
                    &mut let_expr.return_expr,
                    namespaces,
                );
            }
            xpath_ast::ExprSingle::If(if_expr) => {
                self.rewrite_user_function_references_expr(&mut if_expr.condition, namespaces);
                self.rewrite_user_function_references_expr_single(&mut if_expr.then, namespaces);
                self.rewrite_user_function_references_expr_single(&mut if_expr.else_, namespaces);
            }
            xpath_ast::ExprSingle::Binary(binary_expr) => {
                self.rewrite_user_function_references_path_expr(&mut binary_expr.left, namespaces);
                self.rewrite_user_function_references_path_expr(&mut binary_expr.right, namespaces);
            }
            xpath_ast::ExprSingle::For(for_expr) => {
                self.rewrite_user_function_references_expr_single(
                    &mut for_expr.var_expr,
                    namespaces,
                );
                self.rewrite_user_function_references_expr_single(
                    &mut for_expr.return_expr,
                    namespaces,
                );
            }
            xpath_ast::ExprSingle::Quantified(quantified_expr) => {
                self.rewrite_user_function_references_expr_single(
                    &mut quantified_expr.var_expr,
                    namespaces,
                );
                self.rewrite_user_function_references_expr_single(
                    &mut quantified_expr.satisfies_expr,
                    namespaces,
                );
            }
        }
    }

    fn rewrite_user_function_references_path_expr(
        &mut self,
        path_expr: &mut xpath_ast::PathExpr,
        namespaces: &[ast::LiteralNamespace],
    ) {
        for step in &mut path_expr.steps {
            self.rewrite_user_function_references_step_expr(step, namespaces);
        }
    }

    fn rewrite_user_function_references_step_expr(
        &mut self,
        step: &mut xpath_ast::StepExprS,
        namespaces: &[ast::LiteralNamespace],
    ) {
        match &mut step.value {
            xpath_ast::StepExpr::PrimaryExpr(primary) => {
                let extra_postfixes =
                    self.rewrite_user_function_references_primary_expr(primary, namespaces);
                if !extra_postfixes.is_empty() {
                    step.value = xpath_ast::StepExpr::PostfixExpr {
                        primary: primary.clone(),
                        postfixes: extra_postfixes,
                    };
                }
            }
            xpath_ast::StepExpr::PostfixExpr { primary, postfixes } => {
                let extra_postfixes =
                    self.rewrite_user_function_references_primary_expr(primary, namespaces);
                for postfix in postfixes.iter_mut() {
                    self.rewrite_user_function_references_postfix(postfix, namespaces);
                }
                if !extra_postfixes.is_empty() {
                    let mut new_postfixes = extra_postfixes;
                    new_postfixes.append(postfixes);
                    *postfixes = new_postfixes;
                }
            }
            xpath_ast::StepExpr::AxisStep(axis_step) => {
                for predicate in &mut axis_step.predicates {
                    self.rewrite_user_function_references_expr(predicate, namespaces);
                }
            }
        }
    }

    fn rewrite_user_function_references_primary_expr(
        &mut self,
        primary: &mut xpath_ast::PrimaryExprS,
        namespaces: &[ast::LiteralNamespace],
    ) -> Vec<xpath_ast::Postfix> {
        match &mut primary.value {
            xpath_ast::PrimaryExpr::FunctionCall(function_call) => {
                // Replace static-base-uri() with xs:anyURI("...") at compile time,
                // using the current module's base URI. This ensures imported/included
                // modules resolve relative URIs against their own location, and
                // preserves the xs:anyURI type that static-base-uri() returns.
                if function_call.arguments.is_empty()
                    && function_call.name.value.local_name() == "static-base-uri"
                    && (function_call.name.value.namespace().is_empty()
                        || function_call.name.value.namespace() == FN_NAMESPACE)
                {
                    if let Some(base_uri) = self.current_static_context().static_base_uri() {
                        let base_uri_str = base_uri.to_string();
                        let empty_span = (0..0).into();
                        // Build the string literal argument
                        let string_literal = Spanned::new(
                            xpath_ast::PrimaryExpr::Literal(xpath_ast::Literal::String(
                                base_uri_str,
                            )),
                            empty_span,
                        );
                        let step = Spanned::new(
                            xpath_ast::StepExpr::PrimaryExpr(string_literal),
                            empty_span,
                        );
                        let path = xpath_ast::PathExpr { steps: vec![step] };
                        let arg = Spanned::new(xpath_ast::ExprSingle::Path(path), empty_span);
                        // Rewrite to xs:anyURI("base_uri") constructor call
                        function_call.name = Spanned::new(
                            Name::new(
                                "anyURI".to_string(),
                                XS_NAMESPACE.to_string(),
                                "xs".to_string(),
                            ),
                            empty_span,
                        );
                        function_call.arguments = vec![arg];
                        return Vec::new();
                    }
                }

                self.rewrite_static_accumulator_name(function_call, namespaces);

                for argument in &mut function_call.arguments {
                    self.rewrite_user_function_references_expr_single(argument, namespaces);
                }

                self.rewrite_static_format_number_decimal_format_name(function_call, namespaces);

                let arity = match u8::try_from(function_call.arguments.len()) {
                    Ok(arity) => arity,
                    Err(_) => return Vec::new(),
                };

                if let Some(hidden_name) =
                    self.lookup_xslt_function_var_name(&function_call.name.value, arity)
                {
                    let arguments = function_call.arguments.clone();
                    primary.value = xpath_ast::PrimaryExpr::VarRef(hidden_name);
                    vec![xpath_ast::Postfix::ArgumentList(arguments)]
                } else {
                    Vec::new()
                }
            }
            xpath_ast::PrimaryExpr::NamedFunctionRef(named_function_ref) => {
                if let Some(hidden_name) = self.lookup_xslt_function_var_name(
                    &named_function_ref.name.value,
                    named_function_ref.arity,
                ) {
                    primary.value = xpath_ast::PrimaryExpr::VarRef(hidden_name);
                }
                Vec::new()
            }
            xpath_ast::PrimaryExpr::Expr(expr) => {
                self.rewrite_user_function_references_expr_or_empty(expr, namespaces);
                Vec::new()
            }
            xpath_ast::PrimaryExpr::InlineFunction(inline_function) => {
                self.rewrite_user_function_references_expr_or_empty(
                    &mut inline_function.body,
                    namespaces,
                );
                Vec::new()
            }
            xpath_ast::PrimaryExpr::MapConstructor(map_constructor) => {
                for entry in &mut map_constructor.entries {
                    self.rewrite_user_function_references_expr_single(&mut entry.key, namespaces);
                    self.rewrite_user_function_references_expr_single(&mut entry.value, namespaces);
                }
                Vec::new()
            }
            xpath_ast::PrimaryExpr::ArrayConstructor(array_constructor) => {
                match array_constructor {
                    xpath_ast::ArrayConstructor::Square(expr) => {
                        self.rewrite_user_function_references_expr(expr, namespaces);
                    }
                    xpath_ast::ArrayConstructor::Curly(expr) => {
                        self.rewrite_user_function_references_expr_or_empty(expr, namespaces);
                    }
                }
                Vec::new()
            }
            xpath_ast::PrimaryExpr::UnaryLookup(key_specifier) => {
                self.rewrite_user_function_references_key_specifier(key_specifier, namespaces);
                Vec::new()
            }
            xpath_ast::PrimaryExpr::Literal(_)
            | xpath_ast::PrimaryExpr::VarRef(_)
            | xpath_ast::PrimaryExpr::ContextItem => Vec::new(),
        }
    }

    fn rewrite_user_function_references_postfix(
        &mut self,
        postfix: &mut xpath_ast::Postfix,
        namespaces: &[ast::LiteralNamespace],
    ) {
        match postfix {
            xpath_ast::Postfix::Predicate(expr) => {
                self.rewrite_user_function_references_expr(expr, namespaces);
            }
            xpath_ast::Postfix::ArgumentList(arguments) => {
                for argument in arguments {
                    self.rewrite_user_function_references_expr_single(argument, namespaces);
                }
            }
            xpath_ast::Postfix::Lookup(key_specifier) => {
                self.rewrite_user_function_references_key_specifier(key_specifier, namespaces);
            }
        }
    }

    fn rewrite_user_function_references_key_specifier(
        &mut self,
        key_specifier: &mut xpath_ast::KeySpecifier,
        namespaces: &[ast::LiteralNamespace],
    ) {
        if let xpath_ast::KeySpecifier::Expr(expr) = key_specifier {
            self.rewrite_user_function_references_expr_or_empty(expr, namespaces);
        }
    }

    fn rewrite_static_accumulator_name(
        &mut self,
        function_call: &mut xpath_ast::FunctionCall,
        namespaces: &[ast::LiteralNamespace],
    ) {
        if function_call.arguments.len() != 1 {
            return;
        }
        let namespace = function_call.name.value.namespace();
        if !namespace.is_empty() && namespace != FN_NAMESPACE {
            return;
        }
        let function_name = function_call.name.value.local_name();
        if function_name != "accumulator-before" && function_name != "accumulator-after" {
            return;
        }

        let Some(lexical_qname) = Self::static_string_literal(&function_call.arguments[0]) else {
            return;
        };
        let Some((local_name, namespace_uri)) =
            self.resolve_static_qname_with_default(&lexical_qname, namespaces, "")
        else {
            return;
        };

        let rewritten = if namespace_uri.is_empty() {
            local_name.clone()
        } else {
            format!("Q{{{namespace_uri}}}{local_name}")
        };

        if let Some(literal) = Self::static_string_literal_mut(&mut function_call.arguments[0]) {
            *literal = rewritten;
        }

        let hidden_name = if function_name == "accumulator-before" {
            "xslt-accumulator-before"
        } else {
            "xslt-accumulator-after"
        };
        function_call.name = Spanned::new(
            Name::new(hidden_name.to_string(), FN_NAMESPACE.to_string(), String::new()),
            function_call.name.span,
        );
        function_call.arguments.push(Self::context_item_argument());

        self.referenced_accumulators.insert(OwnedName::new(
            local_name,
            namespace_uri,
            String::new(),
        ));
    }

    fn context_item_argument() -> xpath_ast::ExprSingleS {
        let span = (0..0).into();
        let primary = Spanned::new(xpath_ast::PrimaryExpr::ContextItem, span);
        let step = Spanned::new(xpath_ast::StepExpr::PrimaryExpr(primary), span);
        Spanned::new(xpath_ast::ExprSingle::Path(xpath_ast::PathExpr { steps: vec![step] }), span)
    }

    fn rewrite_static_format_number_decimal_format_name(
        &self,
        function_call: &mut xpath_ast::FunctionCall,
        namespaces: &[ast::LiteralNamespace],
    ) {
        if function_call.arguments.len() != 3 {
            return;
        }
        if function_call.name.value.local_name() != "format-number" {
            return;
        }
        let namespace = function_call.name.value.namespace();
        if !namespace.is_empty() && namespace != FN_NAMESPACE {
            return;
        }

        let Some(lexical_qname) = Self::static_string_literal(&function_call.arguments[2]) else {
            return;
        };
        let Some((local_name, namespace_uri)) =
            self.resolve_static_qname_with_default(&lexical_qname, namespaces, "")
        else {
            return;
        };

        let rewritten = if namespace_uri.is_empty() {
            local_name
        } else {
            format!("Q{{{namespace_uri}}}{local_name}")
        };

        if let Some(literal) = Self::static_string_literal_mut(&mut function_call.arguments[2]) {
            *literal = rewritten;
        }
    }

    fn static_string_literal(expr: &xpath_ast::ExprSingleS) -> Option<String> {
        let xpath_ast::ExprSingle::Path(path_expr) = &expr.value else {
            return None;
        };
        let [step] = path_expr.steps.as_slice() else {
            return None;
        };
        let xpath_ast::StepExpr::PrimaryExpr(primary) = &step.value else {
            return None;
        };
        let xpath_ast::PrimaryExpr::Literal(xpath_ast::Literal::String(value)) = &primary.value
        else {
            return None;
        };
        Some(value.clone())
    }

    fn static_string_literal_mut(expr: &mut xpath_ast::ExprSingleS) -> Option<&mut String> {
        let xpath_ast::ExprSingle::Path(path_expr) = &mut expr.value else {
            return None;
        };
        let [step] = path_expr.steps.as_mut_slice() else {
            return None;
        };
        let xpath_ast::StepExpr::PrimaryExpr(primary) = &mut step.value else {
            return None;
        };
        let xpath_ast::PrimaryExpr::Literal(xpath_ast::Literal::String(value)) = &mut primary.value
        else {
            return None;
        };
        Some(value)
    }

    fn pattern_predicate(
        &mut self,
        expr: &xpath_ast::ExprS,
    ) -> error::SpannedResult<ir::FunctionDefinition> {
        let context_names = self.variables.push_context();
        let bindings = self.xpath(expr, &[])?;
        self.variables.pop_context();
        // a predicate is a function that takes a sequence as an argument and returns
        // a boolean that is true if the sequence matches the predicate
        let name = self.variables.new_name();
        let var_atom = Spanned::new(ir::Atom::Variable(name.clone()), (0..0).into());
        let filter = ir::Expr::PatternPredicate(ir::PatternPredicate {
            context_names: context_names.clone(),
            var_atom,
            expr: Box::new(bindings.expr()),
        });
        let bindings = bindings.bind_expr(&mut self.variables, Spanned::new(filter, (0..0).into()));

        let params = vec![
            ir::Param {
                name: context_names.item,
                type_: None,
                default: None,
                required: false,
                original_name: None,
                tunnel: false,
            },
            ir::Param {
                name: context_names.position,
                type_: None,
                default: None,
                required: false,
                original_name: None,
                tunnel: false,
            },
            ir::Param {
                name: context_names.last,
                type_: None,
                default: None,
                required: false,
                original_name: None,
                tunnel: false,
            },
        ];

        Ok(ir::FunctionDefinition {
            params,
            return_type: None,
            body: Box::new(bindings.expr()),
        })
    }

    /// Compile an XSLT pattern and store it as a NumberPatternDefinition.
    /// Returns the index into the number_patterns vec.
    fn compile_number_pattern(&mut self, pattern: &ast::Pattern) -> error::SpannedResult<usize> {
        let compiled = transform_pattern(&pattern.pattern, |expr| self.pattern_predicate(expr))?;
        let index = self.number_patterns.len();
        self.number_patterns
            .push(ir::NumberPatternDefinition { pattern: compiled });
        Ok(index)
    }
}

enum SortDataType {
    Text,
    Number,
}
