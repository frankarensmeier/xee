//! XSLT AST to IR conversion.
//!
//! This module converts the parsed XSLT abstract syntax tree (from `xee-xslt-ast`)
//! into the intermediate representation (from `xee-ir`) that can be compiled to
//! bytecode by `xee-ir`.
//!
//! The conversion is split across several submodules:
//!
//! - [`preprocess`] — Stylesheet loading, import/include resolution, and namespace collection.
//! - [`declarations`] — Top-level declaration compilation (templates, keys, modes, output, etc.).
//! - [`instructions`] — Sequence constructor and instruction compilation (control flow, sorting,
//!   grouping, iteration, etc.).
//! - [`construction`] — XML node construction (element, attribute, text, comment, PI, namespace).
//! - [`xpath_rewrite`] — XPath expression compilation and AST rewriting (current() focus,
//!   user function references, accumulator names, span offsets).

mod construction;
mod declarations;
mod instructions;
mod preprocess;
mod xpath_rewrite;

use ahash::{HashMap, HashMapExt, HashSetExt};
use icu_properties::{maps, GeneralCategory};
use iri_string::types::{IriAbsoluteString, IriReferenceStr};
use xee_name::{Name, Namespaces, FN_NAMESPACE, XS_NAMESPACE};

use std::collections::HashSet;
use xee_interpreter::{
    context::{DecimalFormatSymbols, StaticContext},
    error,
    interpreter::{self, instruction::RaisedError},
};
use xee_ir::{compile_xslt, ir, Bindings, Variables as IrVariables};
use xee_xpath_ast::{ast as xpath_ast, span::Spanned};
use xee_xslt_ast::ast;
use xot::xmlname::{NameStrInfo, OwnedName};

use crate::dynamic_xpath::XsltDynamicXPathEvaluator;

/// Central state for converting an XSLT AST into IR.
///
/// Holds the variable table, function registry, declaration caches (attribute sets,
/// character maps, etc.), and state flags needed during the recursive AST walk.
/// Each submodule adds an `impl IrConverter` block with methods for its domain.
pub(super) struct IrConverter<'a> {
    pub(super) variables: IrVariables,
    pub(super) static_context: &'a StaticContext,
    pub(super) overridden_static_context: Option<StaticContext>,
    pub(super) initial_mode: ast::ApplyTemplatesModeValue,
    pub(super) xslt_functions: HashMap<(OwnedName, u8), OwnedName>,
    pub(super) xslt_function_counter: usize,
    pub(super) accumulator_declarations: HashMap<OwnedName, PreprocessedDeclaration>,
    pub(super) referenced_accumulators: HashSet<OwnedName>,
    pub(super) namespace_aliases: HashMap<String, String>,
    pub(super) attribute_sets: HashMap<(String, String), Vec<ast::AttributeSet>>,
    pub(super) named_outputs: HashMap<(String, String), (i64, ast::Output)>,
    pub(super) character_maps: HashMap<(String, String), (i64, ast::CharacterMap)>,
    pub(super) resolved_character_maps: HashMap<(String, String), ahash::HashMap<char, String>>,
    pub(super) active_attribute_sets: Vec<(String, String)>,
    pub(super) secondary_result_document_depth: usize,
    pub(super) named_templates_with_absent_context: HashSet<String>,
    pub(super) template_continuation_available: bool,
    pub(super) strip_source_document_whitespace: bool,
    pub(super) number_patterns: Vec<ir::NumberPatternDefinition>,
    /// Track mode declarations with import precedence and visibility for conflict detection.
    pub(super) mode_declarations:
        HashMap<Option<OwnedName>, Vec<(i64, ast::Mode, ir::Mode, usize)>>,
    /// Maps stylesheet URI to its start offset in the virtual concatenated source space.
    pub(super) span_offsets: HashMap<String, usize>,
    /// Current offset to add to AST spans to produce global source offsets.
    pub(super) current_span_offset: usize,
}

/// A single XSLT declaration with its import precedence and source location.
/// Built during the preprocessing phase; consumed during the declaration compilation phase.
#[derive(Debug, Clone)]
pub(super) struct PreprocessedDeclaration {
    pub(super) declaration: ast::Declaration,
    pub(super) import_precedence: i64,
    pub(super) module_path: Vec<usize>,
    pub(super) stylesheet_uri: Option<String>,
}

/// A flattened module — all declarations from one stylesheet file (after
/// resolving its own includes), annotated with the module's import tree position.
#[derive(Debug, Clone)]
pub(super) struct PreprocessedModule {
    pub(super) declarations: Vec<(ast::Declaration, Option<String>)>,
    pub(super) module_path: Vec<usize>,
    pub(super) stylesheet_uri: Option<String>,
}

const ARRAY_NAMESPACE: &str = "http://www.w3.org/2005/xpath-functions/array";
const ERR_NAMESPACE: &str = "http://www.w3.org/2005/xqt-errors";
const MAP_NAMESPACE: &str = "http://www.w3.org/2005/xpath-functions/map";
const MATH_NAMESPACE: &str = "http://www.w3.org/2005/xpath-functions/math";
const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_NAMESPACE: &str = "http://www.w3.org/2000/xmlns/";
const XSI_NAMESPACE: &str = "http://www.w3.org/2001/XMLSchema-instance";
pub(super) const XSLT_NAMESPACE: &str = "http://www.w3.org/1999/XSL/Transform";
pub(super) const SERIALIZATION_NAMESPACE: &str = "http://www.w3.org/2010/xslt-xquery-serialization";

#[derive(Debug, Default)]
pub(super) struct LoadedOutputParameterDocument {
    pub(super) method: Option<String>,
    pub(super) use_character_maps: ahash::HashMap<char, String>,
}

pub(super) fn is_reserved_stylesheet_namespace(namespace: &str) -> bool {
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

pub(super) fn is_xsl_initial_template(name: &OwnedName) -> bool {
    name.namespace() == XSLT_NAMESPACE && name.local_name() == "initial-template"
}

pub(super) fn validate_non_reserved_stylesheet_name(
    name: &OwnedName,
    span: ast::Span,
) -> error::SpannedResult<()> {
    if is_reserved_stylesheet_namespace(name.namespace()) && !is_xsl_initial_template(name) {
        return Err(error::Error::XTSE0080.with_ast_span((span.start..span.end).into()));
    }

    Ok(())
}

/// Accumulates xsl:decimal-format properties across multiple declarations,
/// resolving conflicts by import precedence (XTSE1290) and validating symbol
/// constraints (XTSE1295, XTSE1300) before building the final `DecimalFormatSymbols`.
#[derive(Debug, Clone, Default)]
pub(super) struct DecimalFormatAccumulator {
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


pub(super) fn augment_static_context_with_decimal_formats(
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
                detail: None,

                contexts: Vec::new(),
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

/// Compile a fully-preprocessed list of declarations into a runnable program.
/// This is the main entry point after preprocessing: it builds source chunks
/// for multi-file source mapping, configures decimal formats, runs the
/// AST-to-IR conversion, and compiles the IR to bytecode.
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
    let span_offsets: HashMap<String, usize> = source_chunks
        .iter()
        .map(|(uri, start_offset, _, _)| (uri.clone(), *start_offset))
        .collect();
    augment_static_context_with_decimal_formats(&declarations, &mut static_context)?;
    let mut ir_converter = IrConverter::new(&static_context, initial_mode, span_offsets);
    let declarations = ir_converter.transform(&declarations)?;
    let mut program = compile_xslt(declarations, static_context)?;
    program.set_dynamic_xpath_evaluator(Box::new(XsltDynamicXPathEvaluator::default()));
    program.set_transform_evaluator(Box::new(crate::transform::XsltTransformEvaluator));
    program.set_source(xslt.to_string());
    for (uri, start_offset, end_offset, source) in source_chunks {
        program.add_source_chunk(uri, start_offset, end_offset, source);
    }
    Ok(program)
}

/// Build the list of (uri, start_offset, end_offset, source) chunks that map
/// byte offsets in the virtual concatenated source space back to individual
/// stylesheet files. Used for error reporting with multi-file stylesheets.
pub(super) fn build_source_chunks(
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
    let (declarations, static_context, initial_mode) =
        preprocess::preprocess_stylesheet(static_context, xslt, base_dir, initial_mode)?;
    compile_preprocessed_declarations(xslt, declarations, static_context, initial_mode)
}

/// Parse an XSLT stylesheet and return the intermediate representation (IR)
/// without compiling to bytecode. Useful for debugging and inspecting the
/// compilation pipeline.
pub fn parse_to_ir(
    static_context: StaticContext,
    xslt: &str,
    base_dir: Option<std::path::PathBuf>,
    initial_mode: Option<String>,
) -> error::SpannedResult<ir::Declarations> {
    let (declarations, static_context, _initial_mode) =
        parse_to_ir_with_context(static_context, xslt, base_dir, initial_mode)?;
    let _ = static_context;
    Ok(declarations)
}

/// Parse an XSLT stylesheet to IR and also return the enriched StaticContext
/// and initial mode. This is used by the precompiled stylesheet feature to
/// capture all metadata needed for later compilation.
///
/// Returns (ir_declarations, static_context_with_decimal_formats, initial_mode_string).
/// The initial mode string is None for the unnamed mode, or Some("ns local")
/// for a named mode.
pub fn parse_to_ir_with_context(
    static_context: StaticContext,
    xslt: &str,
    base_dir: Option<std::path::PathBuf>,
    initial_mode: Option<String>,
) -> error::SpannedResult<(ir::Declarations, StaticContext, Option<String>)> {
    let (declarations, mut static_context, initial_mode) =
        preprocess::preprocess_stylesheet(static_context, xslt, base_dir, initial_mode)?;
    augment_static_context_with_decimal_formats(&declarations, &mut static_context)?;
    let mut ir_converter = IrConverter::new(&static_context, initial_mode.clone(), HashMap::new());
    let ir_declarations = ir_converter.transform(&declarations)?;

    let initial_mode_str = match &initial_mode {
        ast::ApplyTemplatesModeValue::Unnamed => None,
        ast::ApplyTemplatesModeValue::EqName(name) => {
            Some(format_eqname(name))
        }
        ast::ApplyTemplatesModeValue::Current => {
            Some("#current".to_string())
        }
    };

    Ok((ir_declarations, static_context, initial_mode_str))
}

/// Format an OwnedName as a serializable string using Clark notation: {namespace}local-name
pub(super) fn format_eqname(name: &OwnedName) -> String {
    let ns = name.namespace();
    let local = name.local_name();
    if ns.is_empty() {
        local.to_string()
    } else {
        format!("{{{}}}{}", ns, local)
    }
}


/// Convert an AST span to a global source span by adding a byte offset.
pub(super) fn adjusted_span(span: ast::Span, offset: usize) -> xpath_ast::Span {
    ((span.start + offset)..(span.end + offset)).into()
}

impl<'a> IrConverter<'a> {
    fn new(
        static_context: &'a StaticContext,
        initial_mode: ast::ApplyTemplatesModeValue,
        span_offsets: HashMap<String, usize>,
    ) -> Self {
        IrConverter {
            variables: IrVariables::new(),
            static_context,
            overridden_static_context: None,
            initial_mode,
            xslt_functions: HashMap::new(),
            xslt_function_counter: 0,
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
            mode_declarations: HashMap::new(),
            span_offsets,
            current_span_offset: 0,
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
        let previous_span_offset = self.current_span_offset;
        if let Some(uri) = &declaration.stylesheet_uri {
            self.current_span_offset = self.span_offsets.get(uri).copied().unwrap_or(0);
            let iri = IriAbsoluteString::try_from(uri.clone()).ok();
            let result = self.with_static_base_uri(iri, f);
            self.current_span_offset = previous_span_offset;
            result
        } else {
            self.current_span_offset = 0;
            let result = f(self);
            self.current_span_offset = previous_span_offset;
            result
        }
    }

    fn current_static_base_uri_string(&self) -> Option<String> {
        self.current_static_context()
            .static_base_uri()
            .map(|uri| uri.to_string())
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
        self.static_function_call_expr_spanned(name, namespace, arity, args, (0..0).into())
    }

    fn static_function_call_expr_spanned(
        &mut self,
        name: &str,
        namespace: &str,
        arity: u8,
        args: Vec<ir::AtomS>,
        span: xee_xpath_ast::ast::Span,
    ) -> ir::Expr {
        ir::Expr::FunctionCall(ir::FunctionCall {
            atom: Spanned::new(
                self.static_function_atom(name, namespace, arity),
                span,
            ),
            args,
        })
    }

    fn simple_content_expr(
        &mut self,
        select_atom: ir::AtomS,
        separator_atom: ir::AtomS,
    ) -> ir::Expr {
        self.simple_content_expr_spanned(select_atom, separator_atom, (0..0).into())
    }

    fn simple_content_expr_spanned(
        &mut self,
        select_atom: ir::AtomS,
        separator_atom: ir::AtomS,
        span: xee_xpath_ast::ast::Span,
    ) -> ir::Expr {
        ir::Expr::FunctionCall(ir::FunctionCall {
            atom: Spanned::new(self.simple_content_atom(), span),
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
}

pub(super) enum SortDataType {
    Text,
    Number,
}

