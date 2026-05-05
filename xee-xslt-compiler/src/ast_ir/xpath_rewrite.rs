//! XPath expression compilation and AST rewriting.
//!
//! Handles compiling embedded XPath expressions within XSLT, including:
//! - Rewriting `current()` calls to use the saved focus variable.
//! - Resolving user-defined XSLT function references to their hidden IR names.
//! - Adjusting AST spans for multi-file source mapping.
//! - Rewriting accumulator-value() and format-number() static arguments.
//! - Compiling match patterns and their predicates.

use xee_name::{Name, FN_NAMESPACE, XS_NAMESPACE};
use xee_interpreter::error;
use xee_ir::{ir, Bindings};
use xee_xpath_ast::{ast as xpath_ast, pattern::transform_pattern, span::Spanned};
use xee_xslt_ast::ast;
use xot::xmlname::{NameStrInfo, OwnedName};

use super::IrConverter;

impl<'a> IrConverter<'a> {
    /// Compile an embedded XPath expression into IR bindings.
    ///
    /// Performs several rewrites before handing the expression to the XPath
    /// compiler: adjusts span offsets for multi-file source mapping, rewrites
    /// `current()` references to use the saved focus variable, and resolves
    /// XSLT user-function references to their internal IR names.
    pub(super) fn expression(&mut self, expression: &ast::Expression) -> error::SpannedResult<Bindings> {
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

    /// If the XPath expression references `current()`, bind the current focus
    /// item to a synthetic variable and rewrite `current()` to a `$var` reference.
    /// Returns the bindings and variable name if rewriting occurred.
    pub(super) fn bind_current_focus_variable(
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
                expr.span,
            )),
            expr.span,
        );
        self.variables
            .insert_var_name_in_current_scope(current_name.clone(), ir_name);
        Ok(Some((Bindings::new(binding), current_name)))
    }

    pub(super) fn rewrite_current_focus_expr(&self, expr: &mut xpath_ast::ExprS, current_name: &Name) -> bool {
        let mut rewritten = false;
        for expr_single in &mut expr.value.0 {
            rewritten |= self.rewrite_current_focus_expr_single(expr_single, current_name);
        }
        rewritten
    }

    pub(super) fn rewrite_current_focus_expr_or_empty(
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

    pub(super) fn rewrite_current_focus_expr_single(
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

    pub(super) fn rewrite_current_focus_path_expr(
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

    pub(super) fn rewrite_current_focus_step_expr(
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

    pub(super) fn rewrite_current_focus_primary_expr(
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

    pub(super) fn rewrite_current_focus_key_specifier(
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

    pub(super) fn xpath(
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

    pub(super) fn offset_xpath_spans_expr(&self, expr: &mut xpath_ast::ExprS, offset: usize) {
        expr.span = Self::offset_xpath_span(expr.span, offset);
        for expr_single in &mut expr.value.0 {
            self.offset_xpath_spans_expr_single(expr_single, offset);
        }
    }

    pub(super) fn offset_xpath_spans_expr_or_empty(&self, expr: &mut xpath_ast::ExprOrEmptyS, offset: usize) {
        expr.span = Self::offset_xpath_span(expr.span, offset);
        if let Some(expr_value) = &mut expr.value {
            expr_value.0.iter_mut().for_each(|expr_single| {
                self.offset_xpath_spans_expr_single(expr_single, offset);
            });
        }
    }

    pub(super) fn offset_xpath_spans_expr_single(&self, expr: &mut xpath_ast::ExprSingleS, offset: usize) {
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

    pub(super) fn offset_xpath_spans_path_expr(&self, path_expr: &mut xpath_ast::PathExpr, offset: usize) {
        for step in &mut path_expr.steps {
            self.offset_xpath_spans_step_expr(step, offset);
        }
    }

    pub(super) fn offset_xpath_spans_step_expr(&self, step: &mut xpath_ast::StepExprS, offset: usize) {
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

    pub(super) fn offset_xpath_spans_primary_expr(
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

    pub(super) fn offset_xpath_spans_postfix(&self, postfix: &mut xpath_ast::Postfix, offset: usize) {
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

    pub(super) fn offset_xpath_spans_key_specifier(
        &self,
        key_specifier: &mut xpath_ast::KeySpecifier,
        offset: usize,
    ) {
        if let xpath_ast::KeySpecifier::Expr(expr) = key_specifier {
            self.offset_xpath_spans_expr_or_empty(expr, offset);
        }
    }

    pub(super) fn offset_xpath_span(span: xpath_ast::Span, offset: usize) -> xpath_ast::Span {
        xpath_ast::Span::new(span.start + offset, span.end + offset)
    }

    pub(super) fn lookup_xslt_function_var_name(&self, name: &OwnedName, arity: u8) -> Option<OwnedName> {
        self.xslt_functions.get(&(name.clone(), arity)).cloned()
    }

    pub(super) fn rewrite_user_function_references_expr(
        &mut self,
        expr: &mut xpath_ast::ExprS,
        namespaces: &[ast::LiteralNamespace],
    ) {
        for expr_single in &mut expr.value.0 {
            self.rewrite_user_function_references_expr_single(expr_single, namespaces);
        }
    }

    pub(super) fn rewrite_user_function_references_expr_or_empty(
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

    pub(super) fn rewrite_user_function_references_expr_single(
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

    pub(super) fn rewrite_user_function_references_path_expr(
        &mut self,
        path_expr: &mut xpath_ast::PathExpr,
        namespaces: &[ast::LiteralNamespace],
    ) {
        for step in &mut path_expr.steps {
            self.rewrite_user_function_references_step_expr(step, namespaces);
        }
    }

    pub(super) fn rewrite_user_function_references_step_expr(
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

    pub(super) fn rewrite_user_function_references_primary_expr(
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
                if let Ok(arity) = named_function_ref.arity.try_into() {
                    if let Some(hidden_name) =
                        self.lookup_xslt_function_var_name(&named_function_ref.name.value, arity)
                    {
                        primary.value = xpath_ast::PrimaryExpr::VarRef(hidden_name);
                    }
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

    pub(super) fn rewrite_user_function_references_postfix(
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

    pub(super) fn rewrite_user_function_references_key_specifier(
        &mut self,
        key_specifier: &mut xpath_ast::KeySpecifier,
        namespaces: &[ast::LiteralNamespace],
    ) {
        if let xpath_ast::KeySpecifier::Expr(expr) = key_specifier {
            self.rewrite_user_function_references_expr_or_empty(expr, namespaces);
        }
    }

    pub(super) fn rewrite_static_accumulator_name(
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

    pub(super) fn context_item_argument() -> xpath_ast::ExprSingleS {
        let span = (0..0).into();
        let primary = Spanned::new(xpath_ast::PrimaryExpr::ContextItem, span);
        let step = Spanned::new(xpath_ast::StepExpr::PrimaryExpr(primary), span);
        Spanned::new(xpath_ast::ExprSingle::Path(xpath_ast::PathExpr { steps: vec![step] }), span)
    }

    pub(super) fn rewrite_static_format_number_decimal_format_name(
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

    pub(super) fn static_string_literal(expr: &xpath_ast::ExprSingleS) -> Option<String> {
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

    pub(super) fn static_string_literal_mut(expr: &mut xpath_ast::ExprSingleS) -> Option<&mut String> {
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

    /// Compile a match-pattern predicate expression for use in IR patterns.
    pub(super) fn pattern_predicate(
        &mut self,
        expr: &xpath_ast::ExprS,
    ) -> error::SpannedResult<ir::FunctionDefinition> {
        let context_names = self.variables.push_context();
        // Rewrite current() calls in pattern predicates so they resolve to the
        // match focus (context item at predicate entry) rather than failing at
        // runtime as an unknown function.
        let mut rewritten_expr = expr.clone();
        let current_focus = self.bind_current_focus_variable(&mut rewritten_expr);
        let bindings = self.xpath(&rewritten_expr, &[]);
        let current_focus = current_focus?;
        if let Some((_, current_name)) = &current_focus {
            self.variables
                .remove_var_name_in_current_scope(current_name);
        }
        let bindings = bindings?;
        let bindings = match current_focus {
            Some((current_bindings, _)) => current_bindings.concat(bindings),
            None => bindings,
        };
        self.variables.pop_context();
        // a predicate is a function that takes a sequence as an argument and returns
        // a boolean that is true if the sequence matches the predicate
        let item_param = self.variables.new_name();
        let position_param = self.variables.new_name();
        let last_param = self.variables.new_name();
        let var_atom = Spanned::new(ir::Atom::Variable(item_param.clone()), expr.span);
        let filter = ir::Expr::PatternPredicate(ir::PatternPredicate {
            context_names: context_names.clone(),
            var_atom,
            expr: Box::new(bindings.expr()),
        });
        let body = ir::Expr::Let(ir::Let {
            name: context_names.item.clone(),
            var_expr: Box::new(Spanned::new(
                ir::Expr::Atom(Spanned::new(
                    ir::Atom::Variable(item_param.clone()),
                    expr.span,
                )),
                expr.span,
            )),
            return_expr: Box::new(Spanned::new(
                ir::Expr::Let(ir::Let {
                    name: context_names.position.clone(),
                    var_expr: Box::new(Spanned::new(
                        ir::Expr::Atom(Spanned::new(
                            ir::Atom::Variable(position_param.clone()),
                            expr.span,
                        )),
                        expr.span,
                    )),
                    return_expr: Box::new(Spanned::new(
                        ir::Expr::Let(ir::Let {
                            name: context_names.last.clone(),
                            var_expr: Box::new(Spanned::new(
                                ir::Expr::Atom(Spanned::new(
                                    ir::Atom::Variable(last_param.clone()),
                                    expr.span,
                                )),
                                expr.span,
                            )),
                            return_expr: Box::new(Spanned::new(filter, expr.span)),
                        }),
                        expr.span,
                    )),
                }),
                expr.span,
            )),
        });

        let params = vec![
            ir::Param {
                name: item_param,
                type_: None,
                default: None,
                required: false,
                original_name: None,
                tunnel: false,
            },
            ir::Param {
                name: position_param,
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
        ];

        Ok(ir::FunctionDefinition {
            declared_name: None,
            params,
            return_type: None,
            body: Box::new(Spanned::new(body, expr.span)),
            static_base_uri: self.current_static_base_uri_string(),
        })
    }

    /// Compile an XSLT pattern and store it as a NumberPatternDefinition.
    /// Returns the index into the number_patterns vec.
    /// Compile an xsl:number count/from pattern into a numbered pattern
    /// definition and return its index in `self.number_patterns`.
    pub(super) fn compile_number_pattern(&mut self, pattern: &ast::Pattern) -> error::SpannedResult<usize> {
        let compiled = transform_pattern(&pattern.pattern, |expr| self.pattern_predicate(expr))?;
        let index = self.number_patterns.len();
        self.number_patterns
            .push(ir::NumberPatternDefinition { pattern: compiled });
        Ok(index)
    }
}
