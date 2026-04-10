use ahash::{HashMap, HashMapExt, HashSetExt};
use icu_properties::{maps, GeneralCategory};
use iri_string::types::{IriAbsoluteString, IriReferenceStr};
use xee_name::{Name, Namespaces, FN_NAMESPACE};

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
use xot::{xmlname::{NameStrInfo, OwnedName}, Xot};

use crate::priority::default_priority;

struct IrConverter<'a> {
    variables: IrVariables,
    static_context: &'a StaticContext,
    overridden_static_context: Option<StaticContext>,
    initial_mode: ast::ApplyTemplatesModeValue,
    xslt_functions: HashMap<(OwnedName, u8), OwnedName>,
    namespace_aliases: HashMap<String, String>,
    attribute_sets: HashMap<(String, String), Vec<ast::AttributeSet>>,
    named_outputs: HashMap<(String, String), (i64, ast::Output)>,
    character_maps: HashMap<(String, String), (i64, ast::CharacterMap)>,
    resolved_character_maps: HashMap<(String, String), ahash::HashMap<char, String>>,
    active_attribute_sets: Vec<(String, String)>,
    secondary_result_document_depth: usize,
    named_templates_with_absent_context: HashSet<String>,
    template_continuation_available: bool,
}

#[derive(Debug, Clone)]
struct PreprocessedDeclaration {
    declaration: ast::Declaration,
    import_precedence: i64,
    module_path: Vec<usize>,
}

#[derive(Debug, Clone)]
struct PreprocessedModule {
    declarations: ast::Declarations,
    module_path: Vec<usize>,
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
        push_decimal_format_field(
            &mut self.percent,
            declaration.percent,
            import_precedence,
        );
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
    augment_static_context_with_decimal_formats(&declarations, &mut static_context)?;
    let mut ir_converter = IrConverter::new(&static_context, initial_mode);
    let declarations = ir_converter.transform(&declarations)?;
    let mut program = compile_xslt(declarations, static_context)?;
    program.set_source(xslt.to_string());
    Ok(program)
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
    let mut static_context = augment_static_context_with_stylesheet_namespaces(static_context, xslt);
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

    // Process xsl:import and xsl:include directives
    let declarations = process_imports_and_includes(
        transform.declarations,
        base_dir,
        stylesheet_version,
        static_variables,
    )?;

    let initial_mode = parse_initial_mode_value(initial_mode)?;
    compile_preprocessed_declarations(xslt, declarations, static_context, initial_mode)
}

fn augment_static_context_with_stylesheet_namespaces(
    static_context: StaticContext,
    xslt: &str,
) -> StaticContext {
    let mut xot = Xot::new();
    let Ok(root) = xot.parse(xslt) else {
        return static_context;
    };
    let Ok(document_element) = xot.document_element(root) else {
        return static_context;
    };

    let mut namespaces = static_context.namespaces().clone();
    for (prefix_id, namespace_id) in xot.namespaces_in_scope(document_element) {
        let prefix = xot.prefix_str(prefix_id);
        let namespace = xot.namespace_str(namespace_id);
        namespaces.add(&[(prefix, namespace)]);
    }

    static_context.clone_with_namespaces(namespaces)
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
) -> error::SpannedResult<Vec<PreprocessedDeclaration>> {
    let (modules, _) = process_stylesheet_module(
        declarations,
        base_dir,
        &mut Vec::new(),
        Vec::new(),
        stylesheet_version,
        StaticVariables::new(),
        &module_static_variables,
    )?;
    let mut result = Vec::new();
    for (import_precedence, module) in modules.into_iter().enumerate() {
        for declaration in module.declarations {
            result.push(PreprocessedDeclaration {
                declaration,
                import_precedence: import_precedence as i64,
                module_path: module.module_path.clone(),
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
) -> error::SpannedResult<(Vec<PreprocessedModule>, StaticVariables)> {
    let mut local_declarations = Vec::new();
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
                ) = load_stylesheet(
                    &import.href.to_string(),
                    base_dir.as_ref(),
                    StaticVariables::new(),
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
                    StaticVariables::new(),
                    &imported_module_static_variables,
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
                )?;
                active_paths.pop();
                in_scope_static_variables = included_static_variables;
                if let Some(included_local_module) = processed.pop() {
                    local_declarations.extend(included_local_module.declarations);
                }
                imports.extend(processed);
            }
            _ => {
                remember_static_global(
                    &decl,
                    module_static_variables,
                    &mut in_scope_static_variables,
                );
                local_declarations.push(decl);
            }
        }
    }

    let mut result = imports;
    result.push(PreprocessedModule {
        declarations: local_declarations,
        module_path,
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
        Some(stylesheet_uri),
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
            namespace_aliases: HashMap::new(),
            attribute_sets: HashMap::new(),
            named_outputs: HashMap::new(),
            character_maps: HashMap::new(),
            resolved_character_maps: HashMap::new(),
            active_attribute_sets: Vec::new(),
            secondary_result_document_depth: 0,
            named_templates_with_absent_context: HashSet::new(),
            template_continuation_available: false,
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
        self.overridden_static_context = Some(base_context.clone_with_static_base_uri(static_base_uri));
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

    fn transform(
        &mut self,
        declarations: &[PreprocessedDeclaration],
    ) -> error::SpannedResult<ir::Declarations> {
        self.register_xslt_function_names(declarations)?;
        self.collect_attribute_sets(declarations);
        self.validate_attribute_set_references()?;
        self.collect_named_outputs(declarations);
        self.collect_character_maps(declarations);
        self.collect_named_templates_with_absent_context(declarations);
        self.collect_namespace_aliases(declarations);
        // Register global variable/param names early so $var references resolve.
        let global_vars = self.collect_global_variables(declarations)?;

        let main_sequence_constructor = self.main_sequence_constructor();
        let main = self.sequence_constructor_function(&main_sequence_constructor)?;
        let mut ir_declarations = ir::Declarations::new(main);
        ir_declarations.global_variables = global_vars;

        for declaration in declarations {
            self.declaration(&mut ir_declarations, declaration)?;
        }

        Ok(ir_declarations)
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
                self.character_maps
                    .insert(key, (declaration.import_precedence, (**character_map).clone()));
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
            return Err(error::Error::Unsupported("Unknown xsl:result-document @format".to_string()).into());
        };
        Ok(output.clone())
    }

    fn output_method_literal(
        method: Option<&ast::OutputMethod>,
    ) -> error::SpannedResult<Option<String>> {
        Ok(match method {
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
            "yes" | "true" | "1" | "no" | "false" | "0" | "omit" => {
                Ok(trimmed.to_string())
            }
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

    fn qname_list_literal(names: &[ast::EqName]) -> String {
        names.iter()
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
                template.context_item.as_ref().and_then(|context_item| context_item.use_.as_ref()),
                Some(ast::Use::Absent)
            );
            if has_absent_context {
                self.named_templates_with_absent_context
                    .insert(name.local_name().to_string());
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
            // These declarations are parsed but not yet compiled - skip gracefully
            // to allow stylesheets containing them to still process templates
            Function(_) | Variable(_) | Param(_) | Key(_) | StripSpace(_) | PreserveSpace(_)
            | DecimalFormat(_) | CharacterMap(_) | NamespaceAlias(_) | ImportSchema(_)
            | UsePackage(_) | GlobalContextItem(_) | Accumulator(_) => Ok(()),
        }
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
                    let expr = self.with_hidden_global_name(&var.name, |this| {
                        let context_names = this.variables.push_context();
                        let params = Self::context_params(&context_names);
                        let expr = this.global_variable_expr(
                            var.select.as_ref(),
                            &var.sequence_constructor,
                            var.as_.as_ref(),
                        )?;
                        this.variables.pop_context();
                        Ok((params, expr))
                    })?;
                    globals.push(ir::GlobalVariable {
                        name,
                        original_name: Some(var.name.clone()),
                        external: false,
                        required: false,
                        params: expr.0,
                        expr: expr.1,
                    });
                }
                ast::Declaration::Param(param) => {
                    self.validate_param(param)?;
                    let name = self.variables.lookup_var_name(&param.name).unwrap();
                    let expr = self.with_hidden_global_name(&param.name, |this| {
                        let context_names = this.variables.push_context();
                        let params = Self::context_params(&context_names);
                        let expr = this.global_param_expr(
                            param.select.as_ref(),
                            &param.sequence_constructor,
                        )?;
                        this.variables.pop_context();
                        Ok((params, expr))
                    })?;
                    globals.push(ir::GlobalVariable {
                        name,
                        original_name: Some(param.name.clone()),
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
                    let context_names = self.variables.push_context();
                    let params = Self::context_params(&context_names);
                    let function_definition = self.xslt_function_definition(function)?;
                    self.variables.pop_context();
                    let expr = Spanned::new(
                        ir::Expr::FunctionDefinition(function_definition),
                        (function.span.start..function.span.end).into(),
                    );
                    globals.push(ir::GlobalVariable {
                        name,
                        original_name: None,
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
            ast::SequenceConstructorItem::Content(ast::Content::Text(text)) => !text.trim().is_empty(),
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
            self.sequence_constructor(&with_param.sequence_constructor)?
        };

        let bindings = self.convert_bindings(bindings, with_param.as_.as_ref(), RaisedError::XTTE0570)?;
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
            template.context_item.as_ref().and_then(|context_item| context_item.use_.as_ref()),
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

        let bindings = self.sequence_constructor(&function.sequence_constructor)?;

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
                        .sequence_constructor(&ast_param.sequence_constructor)?
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
            serialization.use_character_maps = self.resolve_character_maps(&output.use_character_maps)?;
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
            ValueOf(value_of) => self.value_of(value_of),
            If(if_) => self.if_(if_),
            Choose(choose) => self.choose(choose),
            ForEach(for_each) => self.for_each(for_each),
            ForEachGroup(for_each_group) => self.for_each_group(for_each_group),
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
            _ => Err(error::Error::Unsupported(format!(
                "Instruction not supported: {:?}",
                instruction
            ))
            .into()),
        }
    }

    fn message(&mut self, message: &ast::Message) -> error::SpannedResult<Bindings> {
        let empty_sequence = self.empty_sequence();
        let message_bindings = if let Some(select) = &message.select {
            self.expression(select)?
        } else if !message.sequence_constructor.is_empty() {
            self.sequence_constructor(&message.sequence_constructor)?
        } else {
            Bindings::new(
                self.variables
                    .new_binding_no_span(empty_sequence.value.clone()),
            )
        };

        Ok(message_bindings.bind_expr(&mut self.variables, empty_sequence))
    }

    fn result_document(
        &mut self,
        result_document: &ast::ResultDocument,
    ) -> error::SpannedResult<Bindings> {
        if matches!(
            result_document.validation,
            Some(ast::Validation::Strict | ast::Validation::Lax | ast::Validation::Preserve)
        )
            || result_document.type_.is_some()
            || result_document.allow_duplicate_names.is_some()
            || result_document.build_tree.is_some()
            || result_document.escape_uri_attributes.is_some()
            || result_document.item_separator.is_some()
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

        let formatted_output = match &result_document.format {
            Some(format) => Some(self.resolve_named_output(format, &result_document.namespaces)?),
            None => None,
        };

        let method_literal = if let Some(method) = &result_document.method {
            self.static_value_template(method).ok_or_else(|| {
                error::SpannedError {
                    error: error::Error::Unsupported(
                        "Dynamic xsl:result-document @method is not supported yet".to_string(),
                    ),
                    span: Some((result_document.span.start..result_document.span.end).into()),
                }
            })?
        } else if let Some(output) = formatted_output.as_ref() {
            Self::output_method_literal(output.method.as_ref())?.unwrap_or_default()
        } else {
            String::new()
        };
        let method_atom = Spanned::new(ir::Atom::Const(ir::Const::String(method_literal)), (0..0).into());

        let byte_order_mark_literal = if let Some(byte_order_mark) = &result_document.bye_order_mark {
            let byte_order_mark_literal = self.static_value_template(byte_order_mark).ok_or_else(|| {
                error::SpannedError {
                    error: error::Error::Unsupported(
                        "Dynamic xsl:result-document @byte-order-mark is not supported yet"
                            .to_string(),
                    ),
                    span: Some((result_document.span.start..result_document.span.end).into()),
                }
            })?;
            Self::validate_boolean_literal(&byte_order_mark_literal)?
        } else if let Some(output) = formatted_output.as_ref() {
            output.byte_order_mark.to_string()
        } else {
            String::new()
        };
        let byte_order_mark_atom = Spanned::new(
            ir::Atom::Const(ir::Const::String(byte_order_mark_literal)),
            (0..0).into(),
        );

        let cdata_literal = if let Some(cdata_section_elements) = &result_document.cdata_section_elements {
            let direct_literal = self.static_value_template(cdata_section_elements).ok_or_else(|| {
                error::SpannedError {
                    error: error::Error::Unsupported(
                        "Dynamic xsl:result-document @cdata-section-elements is not supported yet".to_string(),
                    ),
                    span: Some((result_document.span.start..result_document.span.end).into()),
                }
            })?;
            if let Some(output) = formatted_output.as_ref() {
                Self::merge_literal_qname_lists(&output.cdata_section_elements, Some(direct_literal))
            } else {
                direct_literal
            }
        } else if let Some(output) = formatted_output.as_ref() {
            Self::qname_list_literal(&output.cdata_section_elements)
        } else {
            String::new()
        };
        let cdata_atom = Spanned::new(ir::Atom::Const(ir::Const::String(cdata_literal)), (0..0).into());

        let doctype_public_literal = if let Some(doctype_public) = &result_document.doctype_public {
            self.static_value_template(doctype_public).ok_or_else(|| {
                error::SpannedError {
                    error: error::Error::Unsupported(
                        "Dynamic xsl:result-document @doctype-public is not supported yet".to_string(),
                    ),
                    span: Some((result_document.span.start..result_document.span.end).into()),
                }
            })?
        } else if let Some(output) = formatted_output.as_ref() {
            output.doctype_public.clone().unwrap_or_default()
        } else {
            String::new()
        };
        let doctype_public_atom =
            Spanned::new(ir::Atom::Const(ir::Const::String(doctype_public_literal)), (0..0).into());

        let doctype_system_literal = if let Some(doctype_system) = &result_document.doctype_system {
            self.static_value_template(doctype_system).ok_or_else(|| {
                error::SpannedError {
                    error: error::Error::Unsupported(
                        "Dynamic xsl:result-document @doctype-system is not supported yet".to_string(),
                    ),
                    span: Some((result_document.span.start..result_document.span.end).into()),
                }
            })?
        } else if let Some(output) = formatted_output.as_ref() {
            output.doctype_system.clone().unwrap_or_default()
        } else {
            String::new()
        };
        let doctype_system_atom =
            Spanned::new(ir::Atom::Const(ir::Const::String(doctype_system_literal)), (0..0).into());

        let include_content_type_literal = if let Some(include_content_type) = &result_document.include_content_type {
            self.static_value_template(include_content_type).ok_or_else(|| {
                error::SpannedError {
                    error: error::Error::Unsupported(
                        "Dynamic xsl:result-document @include-content-type is not supported yet"
                            .to_string(),
                    ),
                    span: Some((result_document.span.start..result_document.span.end).into()),
                }
            })?
        } else if let Some(output) = formatted_output.as_ref() {
            output.include_content_type.to_string()
        } else {
            String::new()
        };
        let include_content_type_atom = Spanned::new(
            ir::Atom::Const(ir::Const::String(include_content_type_literal)),
            (0..0).into(),
        );

        let media_type_literal = if let Some(media_type) = &result_document.media_type {
            self.static_value_template(media_type).ok_or_else(|| {
                error::SpannedError {
                    error: error::Error::Unsupported(
                        "Dynamic xsl:result-document @media-type is not supported yet".to_string(),
                    ),
                    span: Some((result_document.span.start..result_document.span.end).into()),
                }
            })?
        } else if let Some(output) = formatted_output.as_ref() {
            output.media_type.clone().unwrap_or_default()
        } else {
            String::new()
        };
        let media_type_atom =
            Spanned::new(ir::Atom::Const(ir::Const::String(media_type_literal)), (0..0).into());

        let omit_xml_declaration_literal =
            if let Some(omit_xml_declaration) = &result_document.omit_xml_declaration {
                let omit_xml_declaration_literal = self.static_value_template(omit_xml_declaration).ok_or_else(|| {
                    error::SpannedError {
                        error: error::Error::Unsupported(
                            "Dynamic xsl:result-document @omit-xml-declaration is not supported yet"
                                .to_string(),
                        ),
                        span: Some((result_document.span.start..result_document.span.end).into()),
                    }
                })?;
                Self::validate_boolean_literal(&omit_xml_declaration_literal)?
            } else if let Some(output) = formatted_output.as_ref() {
                output.omit_xml_declaration.to_string()
            } else {
                String::new()
            };
        let omit_xml_declaration_atom = Spanned::new(
            ir::Atom::Const(ir::Const::String(omit_xml_declaration_literal)),
            (0..0).into(),
        );

        let standalone_literal = if let Some(standalone) = &result_document.standalone {
            let standalone_literal = self.static_value_template(standalone).ok_or_else(|| {
                error::SpannedError {
                    error: error::Error::Unsupported(
                        "Dynamic xsl:result-document @standalone is not supported yet".to_string(),
                    ),
                    span: Some((result_document.span.start..result_document.span.end).into()),
                }
            })?;
            Self::validate_standalone_literal(&standalone_literal)?
        } else if let Some(output) = formatted_output.as_ref() {
            Self::output_standalone_literal(output.standalone.as_ref()).unwrap_or_default()
        } else {
            String::new()
        };
        let standalone_atom =
            Spanned::new(ir::Atom::Const(ir::Const::String(standalone_literal)), (0..0).into());

        let (html_version_atom, html_version_bindings) = if let Some(html_version) = &result_document.html_version {
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
                Spanned::new(ir::Atom::Const(ir::Const::String(String::new())), (0..0).into()),
                Bindings::empty(),
            )
        };

        let use_character_maps_literal = if let Some(use_character_maps) = &result_document.use_character_maps {
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

        let version_literal = if let Some(version) = &result_document.version {
            self.static_value_template(version).ok_or_else(|| {
                error::SpannedError {
                    error: error::Error::Unsupported(
                        "Dynamic xsl:result-document @output-version is not supported yet"
                            .to_string(),
                    ),
                    span: Some((result_document.span.start..result_document.span.end).into()),
                }
            })?
        } else if let Some(output) = formatted_output.as_ref() {
            output.version.clone().unwrap_or_default()
        } else {
            String::new()
        };
        let version_atom =
            Spanned::new(ir::Atom::Const(ir::Const::String(version_literal)), (0..0).into());

        if let Some(href) = &result_document.href {
            let (href_atom, href_bindings) = self.attribute_value_template(href)?.atom_bindings();
            let bindings = href_bindings.concat(content_bindings).concat(html_version_bindings);
            let expr = self.static_function_call_expr(
                "store-result-document",
                FN_NAMESPACE,
                2,
                vec![href_atom, content_atom],
            );
            return Ok(bindings.bind_expr(
                &mut self.variables,
                Spanned::new(expr, (result_document.span.start..result_document.span.end).into()),
            ));
        }

        let expr = self.static_function_call_expr(
            "store-principal-result-document",
            FN_NAMESPACE,
            13,
            vec![
                content_atom,
                method_atom,
                byte_order_mark_atom,
                cdata_atom,
                doctype_public_atom,
                doctype_system_atom,
                include_content_type_atom,
                media_type_atom,
                omit_xml_declaration_atom,
                standalone_atom,
                html_version_atom,
                use_character_maps_atom,
                version_atom,
            ],
        );
        Ok(content_bindings.concat(html_version_bindings).bind_expr(
            &mut self.variables,
            Spanned::new(expr, (result_document.span.start..result_document.span.end).into()),
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

        let (atom, bindings) = self.attribute_set_bindings(use_attribute_sets)?.atom_bindings();
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

                let attribute_sets = this.attribute_sets.get(&key).cloned().ok_or(error::Error::XTSE0710)?;

                this.active_attribute_sets.push(key);
                for attribute_set in attribute_sets {
                    let attribute_set_base_uri = attribute_set
                        .xml_base
                        .as_deref()
                        .and_then(|uri| this.resolve_static_base_uri(uri));
                    this.with_static_base_uri(attribute_set_base_uri, |this| {
                        if let Some(nested) = &attribute_set.use_attribute_sets {
                            let nested_bindings = this.attribute_set_bindings_with_active(nested)?;
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
        self.continue_template(apply_imports.with_params.iter(), ir::ContinueBehavior::ApplyImports)
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
            self.sequence_constructor(&sort.sequence_constructor)?
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
        if let Some(collation) = &sort.collation {
            Ok(self.attribute_value_template(collation)?.atom_bindings())
        } else {
            Ok((
                Spanned::new(ir::Atom::Const(ir::Const::EmptySequence), (0..0).into()),
                Bindings::empty(),
            ))
        }
    }

    fn ensure_supported_sort(&self, sort: &ast::Sort) -> error::SpannedResult<()> {
        if sort.lang.is_some() {
            return Err(error::Error::Unsupported(String::from(
                "xsl:sort lang is not supported yet",
            ))
            .into());
        }
        if sort.case_order.is_some() {
            return Err(error::Error::Unsupported(String::from(
                "xsl:sort case-order is not supported yet",
            ))
            .into());
        }
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
            return Err(error::Error::XTSE0010.with_ast_span(
                (try_.span.start..try_.span.end).into(),
            ));
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
        let (text_atom, bindings) = self
            .select_or_sequence_constructor_simple_content_with_separator(
                value_of,
                &value_of.separator,
            )?
            .atom_bindings();

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

        if sequence_constructor.is_empty() {
            return Ok(document_bindings);
        }

        let (child_atom, child_bindings) = self
            .sequence_constructor(sequence_constructor)?
            .atom_bindings();
        let append_expr = ir::Expr::XmlAppend(ir::XmlAppend {
            parent: document_atom,
            child: child_atom,
        });
        Ok(document_bindings
            .concat(child_bindings)
            .bind_expr_no_span(&mut self.variables, append_expr))
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
        let return_bindings = self
            .with_template_continuation_availability(false, |this| {
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

    fn for_each_group(
        &mut self,
        for_each_group: &ast::ForEachGroup,
    ) -> error::SpannedResult<Bindings> {
        if for_each_group.group_adjacent.is_some()
            || for_each_group.group_starting_with.is_some()
            || for_each_group.group_ending_with.is_some()
            || for_each_group.composite
            || for_each_group.collation.is_some()
            || !for_each_group.sort.is_empty()
        {
            return Err(error::Error::Unsupported(format!(
                "Instruction not supported: {:?}",
                for_each_group
            ))
            .into());
        }

        let group_by = for_each_group.group_by.as_ref().ok_or_else(|| {
            error::Error::Unsupported(format!("Instruction not supported: {:?}", for_each_group))
        })?;

        let (select_atom, bindings) = self.expression(&for_each_group.select)?.atom_bindings();
        let (key_function_atom, key_function_bindings) = self.group_key_function(group_by)?;
        let grouped_expr = self.static_function_call_expr(
            "group-by-first",
            FN_NAMESPACE,
            2,
            vec![select_atom, key_function_atom],
        );
        let (grouped_atom, group_bindings) = key_function_bindings
            .bind_expr_no_span(&mut self.variables, grouped_expr)
            .atom_bindings();
        let bindings = bindings.concat(group_bindings);

        let context_names = self.variables.push_context();
        let return_bindings = self
            .with_template_continuation_availability(false, |this| {
                this.sequence_constructor(&for_each_group.sequence_constructor)
            })?;
        self.variables.pop_context();
        let expr = ir::Expr::Map(ir::Map {
            context_names,
            var_atom: grouped_atom,
            return_expr: Box::new(return_bindings.expr()),
        });

        Ok(bindings.bind_expr_no_span(&mut self.variables, expr))
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
        // TODO: work on document check
        // let _is_document_expr = self.is_document_expr(context_atom.clone());
        let is_element_expr = self.is_element_expr(copy_atom.clone());
        let (is_element_atom, bindings) = bindings
            .bind_expr_no_span(&mut self.variables, is_element_expr)
            .atom_bindings();

        let copy_expr = ir::Expr::Atom(copy_atom.clone());

        let attribute_set_bindings = self.attribute_set_bindings(
            copy.use_attribute_sets.as_deref().unwrap_or(&[]),
        )?;

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
        let (append_content_atom, append_content_bindings) = append_content_bindings.atom_bindings();

        let bindings = bindings.concat(append_content_bindings);

        let append = ir::Expr::XmlAppend(ir::XmlAppend {
            parent: copy_atom,
            child: append_content_atom,
        });

        let if_expr = ir::Expr::If(ir::If {
            condition: is_element_atom,
            then: Box::new(Spanned::new(append, (0..0).into())),
            else_: Box::new(Spanned::new(copy_expr, (0..0).into())),
        });

        Ok(bindings.bind_expr_no_span(&mut self.variables, if_expr))
    }

    // fn is_document_expr(&self, atom: ir::AtomS) -> ir::Expr {
    //     ir::Expr::InstanceOf(ir::InstanceOf {
    //         atom,
    //         sequence_type: xpath_ast::SequenceType::Item(xpath_ast::Item {
    //             item_type: xpath_ast::ItemType::KindTest(xpath_ast::KindTest::Document(None)),
    //             occurrence: xpath_ast::Occurrence::One,
    //         }),
    //     })
    // }

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
        let (localname_atom, bindings) = if let Some((local_name, namespace_uri)) = literal_name
            .as_deref()
            .and_then(|literal_name| {
                self.resolve_static_qname_with_default(literal_name, namespaces, default_namespace)
            })
        {
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
            let (lexical_name_atom, bindings) = self.attribute_value_template(name)?.atom_bindings();
            let default_namespace_atom = Spanned::new(
                ir::Atom::Const(ir::Const::String(default_namespace.to_string())),
                (0..0).into(),
            );
            let namespace_map_atom = Spanned::new(
                ir::Atom::Const(ir::Const::String(self.encode_literal_namespaces(namespaces))),
                (0..0).into(),
            );
            let (force_namespace_atom, namespace_bindings) = if let Some(namespace) = namespace {
                self.attribute_value_template(namespace)?.atom_bindings()
            } else {
                (
                    Spanned::new(ir::Atom::Const(ir::Const::String(String::new())), (0..0).into()),
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
            .unwrap_or_else(|| self.current_static_context().namespaces().default_element_namespace().to_string())
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
        let (element_atom, bindings) = if let Some(namespace) =
            self.static_name_namespace(
                &element.name,
                &element.namespace,
                &element.namespaces,
                &default_namespace,
            )
        {
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

        let (href_atom, href_bindings) = self.attribute_value_template(&source_document.href)?.atom_bindings();
        let load_expr = self.static_function_call_expr("doc", FN_NAMESPACE, 1, vec![href_atom]);
        let load_bindings = href_bindings.bind_expr(
            &mut self.variables,
            Spanned::new(load_expr, (source_document.span.start..source_document.span.end).into()),
        );
        let (document_atom, bindings) = load_bindings.atom_bindings();

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
            Spanned::new(expr, (source_document.span.start..source_document.span.end).into()),
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
            .xml_name_dynamic(&attribute.name, &attribute.namespace, &attribute.namespaces, "")?
            .atom_bindings();
        let (value_atom, value_bindings) = self
            .select_or_sequence_constructor(attribute)?
            .atom_bindings();
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
        let (text_atom, text_bindings) = self
            .select_or_sequence_constructor_simple_content(namespace)?
            .atom_bindings();
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
        let (atom, bindings) = self
            .select_or_sequence_constructor_simple_content(comment)?
            .atom_bindings();
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
        let (content_atom, content_bindings) = self
            .select_or_sequence_constructor_simple_content(pi)?
            .atom_bindings();
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

    fn static_base_uri_literal(&self, expression: &ast::Expression) -> Option<String> {
        let base_uri = self.current_static_context().static_base_uri()?.to_string();
        let exprsingles = &expression.xpath.0.value.0;
        let [exprsingle] = exprsingles.as_slice() else {
            return None;
        };
        let xpath_ast::ExprSingle::Path(path) = &exprsingle.value else {
            return None;
        };
        let [step] = path.steps.as_slice() else {
            return None;
        };
        let xpath_ast::StepExpr::PrimaryExpr(primary) = &step.value else {
            return None;
        };
        let xpath_ast::PrimaryExpr::FunctionCall(function_call) = &primary.value else {
            return None;
        };
        if !function_call.arguments.is_empty() {
            return None;
        }
        if function_call.name.value.local_name() != "static-base-uri" {
            return None;
        }
        let namespace = function_call.name.value.namespace();
        if !namespace.is_empty() && namespace != FN_NAMESPACE {
            return None;
        }
        Some(base_uri)
    }

    fn expression(&mut self, expression: &ast::Expression) -> error::SpannedResult<Bindings> {
        if let Some(base_uri) = self.static_base_uri_literal(expression) {
            let atom = Spanned::new(ir::Atom::Const(ir::Const::String(base_uri)), (0..0).into());
            return Ok(Bindings::empty().bind_expr_no_span(&mut self.variables, ir::Expr::Atom(atom)));
        }
        let mut rewritten_xpath = expression.xpath.0.clone();
        self.offset_xpath_spans_expr(&mut rewritten_xpath, expression.span.start);
        let current_focus = self.bind_current_focus_variable(&mut rewritten_xpath);
        self.rewrite_user_function_references_expr(&mut rewritten_xpath, &expression.namespaces);
        let static_context = self
            .current_static_context()
            .clone_with_static_base_uri(
                self.current_static_context()
                    .static_base_uri()
                    .map(ToOwned::to_owned),
            );
        let mut ir_converter =
            xee_xpath_compiler::IrConverter::new(&mut self.variables, &static_context);
        let bindings = ir_converter.expr(&rewritten_xpath);
        let current_focus = current_focus?;
        if let Some((_, current_name)) = &current_focus {
            self.variables.remove_var_name_in_current_scope(current_name);
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
                    | self.rewrite_current_focus_expr_single(&mut let_expr.return_expr, current_name)
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
                    | self.rewrite_current_focus_expr_single(&mut for_expr.return_expr, current_name)
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
                                rewritten |= self
                                    .rewrite_current_focus_expr_single(argument, current_name);
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
                    rewritten |= self.rewrite_current_focus_expr_single(&mut entry.key, current_name);
                    rewritten |=
                        self.rewrite_current_focus_expr_single(&mut entry.value, current_name);
                }
                rewritten
            }
            xpath_ast::PrimaryExpr::ArrayConstructor(array_constructor) => match array_constructor {
                xpath_ast::ArrayConstructor::Square(expr) => {
                    self.rewrite_current_focus_expr(expr, current_name)
                }
                xpath_ast::ArrayConstructor::Curly(expr) => {
                    self.rewrite_current_focus_expr_or_empty(expr, current_name)
                }
            },
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
        let static_context = self
            .current_static_context()
            .clone_with_static_base_uri(
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
            xpath_ast::PrimaryExpr::ArrayConstructor(array_constructor) => match array_constructor {
                xpath_ast::ArrayConstructor::Square(expr) => {
                    self.offset_xpath_spans_expr(expr, offset);
                }
                xpath_ast::ArrayConstructor::Curly(expr) => {
                    self.offset_xpath_spans_expr_or_empty(expr, offset);
                }
            },
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
        &self,
        expr: &mut xpath_ast::ExprS,
        namespaces: &[ast::LiteralNamespace],
    ) {
        for expr_single in &mut expr.value.0 {
            self.rewrite_user_function_references_expr_single(expr_single, namespaces);
        }
    }

    fn rewrite_user_function_references_expr_or_empty(
        &self,
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
        &self,
        expr: &mut xpath_ast::ExprSingleS,
        namespaces: &[ast::LiteralNamespace],
    ) {
        match &mut expr.value {
            xpath_ast::ExprSingle::Path(path_expr) => {
                self.rewrite_user_function_references_path_expr(path_expr, namespaces);
            }
            xpath_ast::ExprSingle::Apply(apply_expr) => {
                self.rewrite_user_function_references_path_expr(&mut apply_expr.path_expr, namespaces);
                if let xpath_ast::ApplyOperator::SimpleMap(path_exprs) = &mut apply_expr.operator {
                    for path_expr in path_exprs {
                        self.rewrite_user_function_references_path_expr(path_expr, namespaces);
                    }
                }
            }
            xpath_ast::ExprSingle::Let(let_expr) => {
                self.rewrite_user_function_references_expr_single(&mut let_expr.var_expr, namespaces);
                self.rewrite_user_function_references_expr_single(&mut let_expr.return_expr, namespaces);
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
                self.rewrite_user_function_references_expr_single(&mut for_expr.var_expr, namespaces);
                self.rewrite_user_function_references_expr_single(&mut for_expr.return_expr, namespaces);
            }
            xpath_ast::ExprSingle::Quantified(quantified_expr) => {
                self.rewrite_user_function_references_expr_single(&mut quantified_expr.var_expr, namespaces);
                self.rewrite_user_function_references_expr_single(
                    &mut quantified_expr.satisfies_expr,
                    namespaces,
                );
            }
        }
    }

    fn rewrite_user_function_references_path_expr(
        &self,
        path_expr: &mut xpath_ast::PathExpr,
        namespaces: &[ast::LiteralNamespace],
    ) {
        for step in &mut path_expr.steps {
            self.rewrite_user_function_references_step_expr(step, namespaces);
        }
    }

    fn rewrite_user_function_references_step_expr(
        &self,
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
        &self,
        primary: &mut xpath_ast::PrimaryExprS,
        namespaces: &[ast::LiteralNamespace],
    ) -> Vec<xpath_ast::Postfix> {
        match &mut primary.value {
            xpath_ast::PrimaryExpr::FunctionCall(function_call) => {
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
                self.rewrite_user_function_references_expr_or_empty(&mut inline_function.body, namespaces);
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
        &self,
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
        &self,
        key_specifier: &mut xpath_ast::KeySpecifier,
        namespaces: &[ast::LiteralNamespace],
    ) {
        if let xpath_ast::KeySpecifier::Expr(expr) = key_specifier {
            self.rewrite_user_function_references_expr_or_empty(expr, namespaces);
        }
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
        let xpath_ast::PrimaryExpr::Literal(xpath_ast::Literal::String(value)) = &primary.value else {
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
        let xpath_ast::PrimaryExpr::Literal(xpath_ast::Literal::String(value)) = &mut primary.value else {
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
}

enum SortDataType {
    Text,
    Number,
}
