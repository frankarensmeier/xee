use ahash::{HashMap, HashMapExt, HashSetExt};
use iri_string::types::{IriAbsoluteString, IriReferenceStr, IriReferenceString};
use std::cell::RefCell;
use std::rc::Rc;
use xee_interpreter::{
    context::{self, DynamicContext},
    error, function,
    interpreter::{
        DynamicXPathEvaluator, DynamicXPathRequest, InitialFocusMode, Interpreter, Program,
    },
    sequence,
};
use xee_ir::{
    FunctionBuilder, FunctionCompiler, ModeIds, Scopes, TemplateIds, TemplateParams, Variables,
};
use xee_name::{Name, Namespaces, VariableNames};
use xee_xpath_ast::ast as xpath_ast;
use xot::xmlname::{NameStrInfo, OwnedName};
use xot::Xot;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DynamicXPathCacheKey {
    xpath: String,
    default_element_namespace: String,
    default_function_namespace: String,
    namespace_bindings: Vec<(String, String)>,
    variable_names: Vec<(String, String)>,
    default_collation: String,
    base_uri: Option<String>,
    context_item_supplied: bool,
}

/// Key for the fast cache. Includes all fields that affect how an XPath
/// expression is compiled, but uses the namespace context node identity
/// instead of extracting and sorting the full namespace bindings.
/// This avoids the expensive `namespaces_for_request()` tree walk and
/// sorted `Vec<(String, String)>` allocation that the full
/// `DynamicXPathCacheKey` requires.
///
/// When `namespace_context_node` is `Some(node)`, the node identity fully
/// determines the in-scope namespace bindings (xot guarantees this).
/// When `namespace_context_node` is `None`, the prefix bindings come from the
/// static context, which is per-Program and thus identical for all calls
/// within one `XsltDynamicXPathEvaluator` instance. The
/// `xpath_default_namespace` and `default_function_namespace` fields capture
/// the remaining namespace-related variation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FastCacheKey {
    xpath: String,
    variable_names: Vec<(String, String)>,
    context_item_supplied: bool,
    /// Node identity for namespace resolution. Same node → same in-scope
    /// namespaces. None when using the static context's namespaces.
    namespace_context_node: Option<xot::Node>,
    /// Only relevant when namespace_context_node is None.
    xpath_default_namespace: String,
    default_function_namespace: String,
    default_collation: String,
    base_uri: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct XsltDynamicXPathEvaluator {
    cache: RefCell<HashMap<DynamicXPathCacheKey, Rc<Program>>>,
    /// Fast lookup that skips namespace extraction and full key construction.
    /// Maps to `Some(program)` when the key uniquely identifies a single
    /// compiled program, or `None` when the same key has been seen with
    /// different namespace contexts (ambiguous — must fall back to full key).
    fast_cache: RefCell<HashMap<FastCacheKey, Option<Rc<Program>>>>,
}

impl DynamicXPathEvaluator for XsltDynamicXPathEvaluator {
    fn evaluate(
        &self,
        request: &DynamicXPathRequest,
        context: &DynamicContext,
        interpreter: &mut Interpreter<'_>,
    ) -> error::SpannedResult<sequence::Sequence> {
        // Fast path: check if this XPath string + variable names uniquely maps
        // to a cached program. This avoids the expensive namespace extraction,
        // cache key construction, and sorting on the hot path (441K+ calls in
        // typical DocBook transforms, 99.96% cache hits).
        let (variables, variable_names) = variables_for_request(request.with_params.as_ref())?;
        let fast_key = build_fast_cache_key(request, &variable_names, context);
        if let Some(Some(cached_program)) =
            self.fast_cache.borrow().get(&fast_key).cloned()
        {
            let context_item = if request.context_item_supplied {
                request.context_item.clone()
            } else {
                context.context_item().cloned()
            };
            let dynamic_context =
                context.clone_for_program(cached_program.as_ref(), context_item, variables);
            let result = cached_program
                .runnable(&dynamic_context)
                .many(interpreter.xot_mut())?;
            return Ok(result.with_owned_inline_program(cached_program));
        }

        // Slow path: build full cache key with namespace and variable context
        let mut namespaces = namespaces_for_request(
            context,
            request.namespace_context.as_ref(),
            interpreter.xot_mut(),
        )?;
        if request.namespace_context.is_none() {
            namespaces.default_element_namespace = request.xpath_default_namespace.clone();
        }

        // Build cache key and check for a previously compiled program
        let cache_key = build_cache_key(
            &request.xpath,
            &namespaces,
            &variable_names,
            &request.default_collation,
            request.base_uri.as_deref(),
            request.context_item_supplied,
        );

        if let Some(cached_program) = self.cache.borrow().get(&cache_key).cloned() {
            let context_item = if request.context_item_supplied {
                request.context_item.clone()
            } else {
                context.context_item().cloned()
            };
            let dynamic_context =
                context.clone_for_program(cached_program.as_ref(), context_item, variables);
            let result = cached_program
                .runnable(&dynamic_context)
                .many(interpreter.xot_mut())?;
            return Ok(result.with_owned_inline_program(cached_program));
        }

        let mut static_context = context
            .static_context()
            .clone_with_namespaces_and_variables(namespaces, variable_names);
        let default_collation: IriReferenceString = request
            .default_collation
            .clone()
            .try_into()
            .map_err(|_| error::Error::FOCH0002)?;
        static_context.set_default_collation_uri(default_collation);
        if let Some(base_uri) = request.base_uri.as_deref() {
            let base_uri = resolve_dynamic_xpath_base_uri(context, base_uri)?;
            static_context = static_context.clone_with_static_base_uri(Some(base_uri));
        }
        let mut xpath = static_context
            .parse_xpath(&request.xpath)
            .map_err(map_dynamic_xpath_parse_error)?;
        let stylesheet_functions = stylesheet_functions(context, &mut xpath);
        let mut program =
            compile_dynamic_xpath(context, static_context, xpath, stylesheet_functions)
                .map_err(map_dynamic_xpath_static_error)?;
        program.set_dynamic_xpath_evaluator(Box::new(XsltDynamicXPathEvaluator::default()));
        program.set_transform_evaluator(Box::new(crate::transform::XsltTransformEvaluator));
        program.set_initial_focus_mode(if request.context_item_supplied {
            InitialFocusMode::Full
        } else {
            InitialFocusMode::ItemOnly
        });
        program.set_source(request.xpath.clone());

        let context_item = if request.context_item_supplied {
            request.context_item.clone()
        } else {
            context.context_item().cloned()
        };
        let program = Rc::new(program);
        self.cache
            .borrow_mut()
            .insert(cache_key, Rc::clone(&program));
        // Update fast cache: if this key already maps to a different
        // program, mark it as ambiguous (None) so future lookups fall through
        // to the full key path.
        {
            let mut fast = self.fast_cache.borrow_mut();
            match fast.get(&fast_key) {
                None => {
                    // First time seeing this key
                    fast.insert(fast_key, Some(Rc::clone(&program)));
                }
                Some(Some(existing)) if !Rc::ptr_eq(existing, &program) => {
                    // Same fast key, different program — ambiguous
                    fast.insert(fast_key, None);
                }
                _ => {
                    // Already correct or already marked ambiguous
                }
            }
        }
        let dynamic_context = context.clone_for_program(program.as_ref(), context_item, variables);
        let result = program.runnable(&dynamic_context).many(interpreter.xot_mut())?;
        Ok(result.with_owned_inline_program(program))
    }
}

fn build_fast_cache_key(
    request: &DynamicXPathRequest,
    variable_names: &VariableNames,
    context: &DynamicContext,
) -> FastCacheKey {
    let mut var_names: Vec<(String, String)> = variable_names
        .iter()
        .map(|name| {
            (
                name.namespace().to_string(),
                name.local_name().to_string(),
            )
        })
        .collect();
    var_names.sort();

    let namespace_context_node = match &request.namespace_context {
        Some(sequence::Item::Node(node)) => Some(*node),
        _ => None,
    };

    // When namespace_context is a node, namespaces_for_node() leaves
    // default_function_namespace at its default (empty). When None,
    // it comes from the static context.
    let default_function_namespace = match &request.namespace_context {
        Some(sequence::Item::Node(_)) => String::new(),
        _ => context
            .static_context()
            .namespaces()
            .default_function_namespace
            .clone(),
    };

    FastCacheKey {
        xpath: request.xpath.clone(),
        variable_names: var_names,
        context_item_supplied: request.context_item_supplied,
        namespace_context_node,
        xpath_default_namespace: request.xpath_default_namespace.clone(),
        default_function_namespace,
        default_collation: request.default_collation.clone(),
        base_uri: request.base_uri.clone(),
    }
}

fn build_cache_key(
    xpath: &str,
    namespaces: &Namespaces,
    variable_names: &VariableNames,
    default_collation: &str,
    base_uri: Option<&str>,
    context_item_supplied: bool,
) -> DynamicXPathCacheKey {
    let mut namespace_bindings: Vec<(String, String)> = namespaces
        .prefix_iter()
        .map(|(prefix, uri)| (prefix.to_string(), uri.to_string()))
        .collect();
    namespace_bindings.sort();

    let mut var_names: Vec<(String, String)> = variable_names
        .iter()
        .map(|name| {
            (
                name.namespace().to_string(),
                name.local_name().to_string(),
            )
        })
        .collect();
    var_names.sort();

    DynamicXPathCacheKey {
        xpath: xpath.to_string(),
        default_element_namespace: namespaces.default_element_namespace.clone(),
        default_function_namespace: namespaces.default_function_namespace.clone(),
        namespace_bindings,
        variable_names: var_names,
        default_collation: default_collation.to_string(),
        base_uri: base_uri.map(|s| s.to_string()),
        context_item_supplied,
    }
}

fn map_dynamic_xpath_parse_error(error: xee_xpath_ast::ParserError) -> error::SpannedError {
    map_dynamic_xpath_static_error(error.into())
}

fn map_dynamic_xpath_static_error(error: error::SpannedError) -> error::SpannedError {
    let mapped_error = match error.error {
        error::Error::XPST0003
        | error::Error::XPST0017
        | error::Error::XPST0051
        | error::Error::XPST0081
        | error::Error::XPST0081Detail(_) => error::Error::XTDE3160,
        other => other,
    };
    error::SpannedError {
        error: mapped_error,
        span: error.span,
        detail: error.detail,
        contexts: Vec::new(),
    }
}

fn resolve_dynamic_xpath_base_uri(
    context: &DynamicContext,
    base_uri: &str,
) -> error::Result<IriAbsoluteString> {
    let base_uri: &IriReferenceStr = base_uri.try_into().map_err(|_| error::Error::FOXT0002)?;
    Ok(match base_uri.to_iri() {
        Ok(uri) => uri
            .to_string()
            .try_into()
            .map_err(|_| error::Error::FOXT0002)?,
        Err(relative_uri) => {
            let base = context
                .static_context()
                .static_base_uri()
                .ok_or(error::Error::FOXT0002)?;
            relative_uri
                .resolve_against(base)
                .to_string()
                .try_into()
                .map_err(|_| error::Error::FOXT0002)?
        }
    })
}

fn variables_for_request(
    with_params: Option<&function::Map>,
) -> error::Result<(context::Variables, VariableNames)> {
    let mut variables = context::Variables::new();
    let mut variable_names = VariableNames::new();
    let Some(with_params) = with_params else {
        return Ok((variables, variable_names));
    };

    for (key, value) in with_params.entries() {
        let name: Name = match key.clone().try_into() {
            Ok(name) => name,
            Err(_) => return Err(error::Error::XTTE3165),
        };
        variable_names.insert(name.clone());
        variables.insert(name, value.clone());
    }

    Ok((variables, variable_names))
}

#[derive(Debug)]
struct StylesheetFunctionBinding {
    rewrite_name: xpath_ast::Name,
    global_index: u16,
}

fn compile_dynamic_xpath(
    context: &DynamicContext,
    static_context: context::StaticContext,
    xpath: xpath_ast::XPath,
    stylesheet_functions: Vec<StylesheetFunctionBinding>,
) -> error::SpannedResult<Program> {
    let caller_program = context.program();
    let mut variables = Variables::new();
    let mut global_variable_ids = ahash::HashMap::new();
    for binding in stylesheet_functions {
        let ir_name = variables.declare_var_name(&binding.rewrite_name);
        global_variable_ids.insert(ir_name, binding.global_index);
    }

    let mut ir_converter = xee_xpath_compiler::IrConverter::new(&mut variables, &static_context);
    let expr = ir_converter.xpath(&xpath)?.expr();

    let mut program = Program::new(static_context, xpath.0.span);
    program.functions = caller_program.functions.clone();
    program.declarations = caller_program.declarations.clone();
    // Dynamic XPath executes its compiled expression as the program entrypoint.
    // Keeping stylesheet named templates here lets absent-context evaluation
    // re-enter xsl:initial-template instead of running the expression.
    program.declarations.named_templates.clear();

    let mut scopes = Scopes::new();
    let builder = FunctionBuilder::new(&mut program);
    let empty_mode_ids = ModeIds::new();
    let empty_template_ids = TemplateIds::new();
    let empty_template_params = TemplateParams::new();
    let mut compiler = FunctionCompiler::new(
        builder,
        &mut scopes,
        &empty_mode_ids,
        &empty_template_ids,
        &empty_template_params,
        &global_variable_ids,
    );
    compiler.compile_expr(&expr)?;
    Ok(program)
}

fn stylesheet_functions(
    context: &DynamicContext,
    xpath: &mut xpath_ast::XPath,
) -> Vec<StylesheetFunctionBinding> {
    let mut by_public_name = HashMap::new();
    let mut bindings = Vec::new();

    for (index, global) in context
        .program()
        .declarations
        .global_variables
        .iter()
        .enumerate()
    {
        let (Some(public_name), Some(public_arity)) = (&global.public_name, global.public_arity)
        else {
            continue;
        };
        let rewrite_name = xpath_ast::Name::new(
            format!("stylesheet-function-{index}"),
            "urn:xee:internal:dynamic-evaluate-function".to_string(),
            "xee-eval".to_string(),
        );
        by_public_name.insert((public_name.clone(), public_arity), rewrite_name.clone());
        bindings.push(StylesheetFunctionBinding {
            rewrite_name,
            global_index: index as u16,
        });
    }

    rewrite_stylesheet_function_references_expr(&mut xpath.0, &by_public_name);
    bindings
}

fn rewrite_stylesheet_function_references_expr(
    expr: &mut xpath_ast::ExprS,
    stylesheet_functions: &HashMap<(OwnedName, u8), xpath_ast::Name>,
) {
    for expr_single in &mut expr.value.0 {
        rewrite_stylesheet_function_references_expr_single(expr_single, stylesheet_functions);
    }
}

fn rewrite_stylesheet_function_references_expr_or_empty(
    expr: &mut xpath_ast::ExprOrEmptyS,
    stylesheet_functions: &HashMap<(OwnedName, u8), xpath_ast::Name>,
) {
    let Some(expr_value) = &mut expr.value else {
        return;
    };
    for expr_single in &mut expr_value.0 {
        rewrite_stylesheet_function_references_expr_single(expr_single, stylesheet_functions);
    }
}

fn rewrite_stylesheet_function_references_expr_single(
    expr: &mut xpath_ast::ExprSingleS,
    stylesheet_functions: &HashMap<(OwnedName, u8), xpath_ast::Name>,
) {
    match &mut expr.value {
        xpath_ast::ExprSingle::Path(path_expr) => {
            rewrite_stylesheet_function_references_path_expr(path_expr, stylesheet_functions);
        }
        xpath_ast::ExprSingle::Apply(apply_expr) => {
            rewrite_stylesheet_function_references_path_expr(
                &mut apply_expr.path_expr,
                stylesheet_functions,
            );
            if let xpath_ast::ApplyOperator::SimpleMap(path_exprs) = &mut apply_expr.operator {
                for path_expr in path_exprs {
                    rewrite_stylesheet_function_references_path_expr(
                        path_expr,
                        stylesheet_functions,
                    );
                }
            }
        }
        xpath_ast::ExprSingle::Let(let_expr) => {
            rewrite_stylesheet_function_references_expr_single(
                &mut let_expr.var_expr,
                stylesheet_functions,
            );
            rewrite_stylesheet_function_references_expr_single(
                &mut let_expr.return_expr,
                stylesheet_functions,
            );
        }
        xpath_ast::ExprSingle::If(if_expr) => {
            rewrite_stylesheet_function_references_expr(
                &mut if_expr.condition,
                stylesheet_functions,
            );
            rewrite_stylesheet_function_references_expr_single(
                &mut if_expr.then,
                stylesheet_functions,
            );
            rewrite_stylesheet_function_references_expr_single(
                &mut if_expr.else_,
                stylesheet_functions,
            );
        }
        xpath_ast::ExprSingle::Binary(binary_expr) => {
            rewrite_stylesheet_function_references_path_expr(
                &mut binary_expr.left,
                stylesheet_functions,
            );
            rewrite_stylesheet_function_references_path_expr(
                &mut binary_expr.right,
                stylesheet_functions,
            );
        }
        xpath_ast::ExprSingle::For(for_expr) => {
            rewrite_stylesheet_function_references_expr_single(
                &mut for_expr.var_expr,
                stylesheet_functions,
            );
            rewrite_stylesheet_function_references_expr_single(
                &mut for_expr.return_expr,
                stylesheet_functions,
            );
        }
        xpath_ast::ExprSingle::Quantified(quantified_expr) => {
            rewrite_stylesheet_function_references_expr_single(
                &mut quantified_expr.var_expr,
                stylesheet_functions,
            );
            rewrite_stylesheet_function_references_expr_single(
                &mut quantified_expr.satisfies_expr,
                stylesheet_functions,
            );
        }
    }
}

fn rewrite_stylesheet_function_references_path_expr(
    path_expr: &mut xpath_ast::PathExpr,
    stylesheet_functions: &HashMap<(OwnedName, u8), xpath_ast::Name>,
) {
    for step in &mut path_expr.steps {
        rewrite_stylesheet_function_references_step_expr(step, stylesheet_functions);
    }
}

fn rewrite_stylesheet_function_references_step_expr(
    step: &mut xpath_ast::StepExprS,
    stylesheet_functions: &HashMap<(OwnedName, u8), xpath_ast::Name>,
) {
    match &mut step.value {
        xpath_ast::StepExpr::PrimaryExpr(primary) => {
            let extra_postfixes =
                rewrite_stylesheet_function_references_primary_expr(primary, stylesheet_functions);
            if !extra_postfixes.is_empty() {
                step.value = xpath_ast::StepExpr::PostfixExpr {
                    primary: primary.clone(),
                    postfixes: extra_postfixes,
                };
            }
        }
        xpath_ast::StepExpr::PostfixExpr { primary, postfixes } => {
            let extra_postfixes =
                rewrite_stylesheet_function_references_primary_expr(primary, stylesheet_functions);
            for postfix in postfixes.iter_mut() {
                rewrite_stylesheet_function_references_postfix(postfix, stylesheet_functions);
            }
            if !extra_postfixes.is_empty() {
                let mut new_postfixes = extra_postfixes;
                new_postfixes.append(postfixes);
                *postfixes = new_postfixes;
            }
        }
        xpath_ast::StepExpr::AxisStep(axis_step) => {
            for predicate in &mut axis_step.predicates {
                rewrite_stylesheet_function_references_expr(predicate, stylesheet_functions);
            }
        }
    }
}

fn rewrite_stylesheet_function_references_primary_expr(
    primary: &mut xpath_ast::PrimaryExprS,
    stylesheet_functions: &HashMap<(OwnedName, u8), xpath_ast::Name>,
) -> Vec<xpath_ast::Postfix> {
    match &mut primary.value {
        xpath_ast::PrimaryExpr::FunctionCall(function_call) => {
            for argument in &mut function_call.arguments {
                rewrite_stylesheet_function_references_expr_single(argument, stylesheet_functions);
            }

            let arity = match u8::try_from(function_call.arguments.len()) {
                Ok(arity) => arity,
                Err(_) => return Vec::new(),
            };

            if let Some(rewrite_name) =
                stylesheet_functions.get(&(function_call.name.value.clone(), arity))
            {
                let arguments = function_call.arguments.clone();
                primary.value = xpath_ast::PrimaryExpr::VarRef(rewrite_name.clone());
                vec![xpath_ast::Postfix::ArgumentList(arguments)]
            } else {
                Vec::new()
            }
        }
        xpath_ast::PrimaryExpr::NamedFunctionRef(named_function_ref) => {
            if let Ok(arity) = named_function_ref.arity.try_into() {
                if let Some(rewrite_name) =
                    stylesheet_functions.get(&(named_function_ref.name.value.clone(), arity))
                {
                    primary.value = xpath_ast::PrimaryExpr::VarRef(rewrite_name.clone());
                }
            }
            Vec::new()
        }
        xpath_ast::PrimaryExpr::Expr(expr) => {
            rewrite_stylesheet_function_references_expr_or_empty(expr, stylesheet_functions);
            Vec::new()
        }
        xpath_ast::PrimaryExpr::InlineFunction(inline_function) => {
            rewrite_stylesheet_function_references_expr_or_empty(
                &mut inline_function.body,
                stylesheet_functions,
            );
            Vec::new()
        }
        xpath_ast::PrimaryExpr::MapConstructor(map_constructor) => {
            for entry in &mut map_constructor.entries {
                rewrite_stylesheet_function_references_expr_single(
                    &mut entry.key,
                    stylesheet_functions,
                );
                rewrite_stylesheet_function_references_expr_single(
                    &mut entry.value,
                    stylesheet_functions,
                );
            }
            Vec::new()
        }
        xpath_ast::PrimaryExpr::ArrayConstructor(array_constructor) => {
            match array_constructor {
                xpath_ast::ArrayConstructor::Square(expr) => {
                    rewrite_stylesheet_function_references_expr(expr, stylesheet_functions);
                }
                xpath_ast::ArrayConstructor::Curly(expr) => {
                    rewrite_stylesheet_function_references_expr_or_empty(
                        expr,
                        stylesheet_functions,
                    );
                }
            }
            Vec::new()
        }
        xpath_ast::PrimaryExpr::UnaryLookup(key_specifier) => {
            rewrite_stylesheet_function_references_key_specifier(
                key_specifier,
                stylesheet_functions,
            );
            Vec::new()
        }
        xpath_ast::PrimaryExpr::Literal(_)
        | xpath_ast::PrimaryExpr::VarRef(_)
        | xpath_ast::PrimaryExpr::ContextItem => Vec::new(),
    }
}

fn rewrite_stylesheet_function_references_postfix(
    postfix: &mut xpath_ast::Postfix,
    stylesheet_functions: &HashMap<(OwnedName, u8), xpath_ast::Name>,
) {
    match postfix {
        xpath_ast::Postfix::Predicate(expr) => {
            rewrite_stylesheet_function_references_expr(expr, stylesheet_functions);
        }
        xpath_ast::Postfix::ArgumentList(arguments) => {
            for argument in arguments {
                rewrite_stylesheet_function_references_expr_single(argument, stylesheet_functions);
            }
        }
        xpath_ast::Postfix::Lookup(key_specifier) => {
            rewrite_stylesheet_function_references_key_specifier(
                key_specifier,
                stylesheet_functions,
            );
        }
    }
}

fn rewrite_stylesheet_function_references_key_specifier(
    key_specifier: &mut xpath_ast::KeySpecifier,
    stylesheet_functions: &HashMap<(OwnedName, u8), xpath_ast::Name>,
) {
    if let xpath_ast::KeySpecifier::Expr(expr) = key_specifier {
        rewrite_stylesheet_function_references_expr_or_empty(expr, stylesheet_functions);
    }
}

fn namespaces_for_request(
    context: &DynamicContext,
    namespace_context: Option<&sequence::Item>,
    xot: &mut Xot,
) -> error::Result<Namespaces> {
    match namespace_context {
        None => Ok(context.static_context().namespaces().clone()),
        Some(sequence::Item::Node(node)) => Ok(namespaces_for_node(*node, xot)),
        Some(_) => Err(error::Error::type_error(
            "xsl:evaluate namespace-context must be a node",
        )),
    }
}

fn namespaces_for_node(node: xot::Node, xot: &Xot) -> Namespaces {
    let mut namespaces = Namespaces::default();
    for (prefix_id, namespace_id) in xot.namespaces_in_scope(node) {
        let prefix = xot.prefix_str(prefix_id);
        let namespace = xot.namespace_str(namespace_id);
        if prefix.is_empty() {
            namespaces.default_element_namespace = namespace.to_string();
        } else {
            namespaces.add(&[(prefix, namespace)]);
        }
    }
    namespaces
}
