//! XSLT instruction compilation.
//!
//! Compiles sequence constructor instructions into IR: control flow (if/choose),
//! iteration (for-each, for-each-group, iterate), sorting, merging, try/catch,
//! template calls, result documents, xsl:number, xsl:analyze-string,
//! and sequence construction.

use xee_name::{Name, FN_NAMESPACE};

use xee_interpreter::{
    error,
    interpreter::instruction::RaisedError,
};
use xee_ir::{ir, Bindings};
use xee_xpath_ast::{ast as xpath_ast, pattern as xpath_pattern, span::Spanned};
use xee_xslt_ast::ast;
use xot::xmlname::NameStrInfo;

use super::{adjusted_span, IrConverter, SortDataType};

impl<'a> IrConverter<'a> {
    /// Wrap a sequence constructor in a function definition that takes
    /// the standard context parameters (item, position, last).
    pub(super) fn sequence_constructor_function(
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
            declared_name: None,
            params,
            return_type: None,
            body: Box::new(bindings.expr()),
            static_base_uri: self.current_static_base_uri_string(),
        })
    }

    /// Compile a sequence constructor (list of instructions) into IR bindings.
    /// Handles variable scoping and xsl:on-empty/xsl:on-non-empty support.
    pub(super) fn sequence_constructor(
        &mut self,
        sequence_constructor: &[ast::SequenceConstructorItem],
    ) -> error::SpannedResult<Bindings> {
        self.variables.push_scope();
        let has_on_empty = sequence_constructor
            .iter()
            .any(|i| self.is_on_empty_or_on_non_empty(i));
        let result = if has_on_empty {
            self.sequence_constructor_with_on_empty(sequence_constructor)
        } else {
            self.sequence_constructor_in_scope(sequence_constructor)
        };
        self.variables.pop_scope();
        result
    }

    pub(super) fn is_on_empty_or_on_non_empty(&self, item: &ast::SequenceConstructorItem) -> bool {
        matches!(
            item,
            ast::SequenceConstructorItem::Instruction(
                ast::SequenceConstructorInstruction::OnEmpty(_)
                    | ast::SequenceConstructorInstruction::OnNonEmpty(_)
            )
        )
    }

    pub(super) fn sequence_constructor_with_on_empty(
        &mut self,
        items: &[ast::SequenceConstructorItem],
    ) -> error::SpannedResult<Bindings> {
        // Partition items into main content and on-empty/on-non-empty
        let main_items: Vec<_> = items
            .iter()
            .filter(|i| !self.is_on_empty_or_on_non_empty(i))
            .cloned()
            .collect();

        // Compile main content and bind to a variable
        let main_bindings = self.sequence_constructor_in_scope(&main_items)?;
        let (main_atom, main_bindings) = main_bindings.atom_bindings();

        // Check populated: fn:exists(fn:xslt-where-populated($main))
        let wp_expr = self.static_function_call_expr(
            "xslt-where-populated",
            FN_NAMESPACE,
            1,
            vec![main_atom.clone()],
        );
        let wp_bindings = main_bindings.bind_expr_no_span(&mut self.variables, wp_expr);
        let (wp_atom, wp_bindings) = wp_bindings.atom_bindings();

        let exists_expr =
            self.static_function_call_expr("exists", FN_NAMESPACE, 1, vec![wp_atom]);
        let exists_bindings = wp_bindings.bind_expr_no_span(&mut self.variables, exists_expr);
        let (is_pop_atom, mut all_bindings) = exists_bindings.atom_bindings();

        // Walk items in original order, generating conditional output.
        // Main items: output $main (if populated) at the position of the
        // first main item; subsequent main items are skipped since they're
        // already included in $main.
        // On-non-empty: if populated then content else ()
        // On-empty: if not populated then content else ()
        let mut first_main_seen = false;
        let mut result_atom: Option<ir::AtomS> = None;

        for item in items {
            let part_atom = if self.is_on_empty_or_on_non_empty(item) {
                let is_on_empty = matches!(
                    item,
                    ast::SequenceConstructorItem::Instruction(
                        ast::SequenceConstructorInstruction::OnEmpty(_)
                    )
                );

                let content_bindings = self.sequence_constructor_item(item)?;
                let (content_atom, content_bindings) = content_bindings.atom_bindings();
                all_bindings = all_bindings.concat(content_bindings);

                // on-empty: if $is_pop then () else content
                // on-non-empty: if $is_pop then content else ()
                let empty = self.empty_sequence();
                let if_expr = if is_on_empty {
                    ir::Expr::If(ir::If {
                        condition: is_pop_atom.clone(),
                        then: Box::new(empty),
                        else_: Box::new(Spanned::new(
                            ir::Expr::Atom(content_atom),
                            (0..0).into(),
                        )),
                    })
                } else {
                    ir::Expr::If(ir::If {
                        condition: is_pop_atom.clone(),
                        then: Box::new(Spanned::new(
                            ir::Expr::Atom(content_atom),
                            (0..0).into(),
                        )),
                        else_: Box::new(empty),
                    })
                };
                let bind =
                    Bindings::empty().bind_expr_no_span(&mut self.variables, if_expr);
                let (atom, bind) = bind.atom_bindings();
                all_bindings = all_bindings.concat(bind);
                Some(atom)
            } else if !first_main_seen {
                first_main_seen = true;
                // Output $main (if populated) at the position of the first
                // main item.
                let empty = self.empty_sequence();
                let if_expr = ir::Expr::If(ir::If {
                    condition: is_pop_atom.clone(),
                    then: Box::new(Spanned::new(
                        ir::Expr::Atom(main_atom.clone()),
                        (0..0).into(),
                    )),
                    else_: Box::new(empty),
                });
                let bind =
                    Bindings::empty().bind_expr_no_span(&mut self.variables, if_expr);
                let (atom, bind) = bind.atom_bindings();
                all_bindings = all_bindings.concat(bind);
                Some(atom)
            } else {
                // Subsequent main items — already included in $main
                None
            };

            if let Some(part) = part_atom {
                result_atom = Some(if let Some(prev) = result_atom {
                    let comma_expr = ir::Expr::Binary(ir::Binary {
                        left: prev,
                        op: ir::BinaryOperator::Comma,
                        right: part,
                    });
                    let bind = Bindings::empty()
                        .bind_expr_no_span(&mut self.variables, comma_expr);
                    let (atom, bind) = bind.atom_bindings();
                    all_bindings = all_bindings.concat(bind);
                    atom
                } else {
                    part
                });
            }
        }

        if let Some(result) = result_atom {
            Ok(all_bindings.bind_expr_no_span(
                &mut self.variables,
                ir::Expr::Atom(result),
            ))
        } else {
            let empty = self.empty_sequence();
            Ok(Bindings::new(
                self.variables.new_binding(empty.value, empty.span),
            ))
        }
    }

    pub(super) fn sequence_constructor_in_scope(
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

    pub(super) fn sequence_constructor_item(
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

    /// Main dispatch: compile a single XSLT instruction into IR bindings.
    pub(super) fn sequence_constructor_instruction(
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
            OnEmpty(on_empty) => self.on_empty_content(on_empty),
            OnNonEmpty(on_non_empty) => self.on_non_empty_content(on_non_empty),
            _ => Err(error::Error::Unsupported(format!(
                "Instruction not supported: {:?}",
                instruction
            ))
            .into()),
        }
    }

    pub(super) fn map(&mut self, map: &ast::Map) -> error::SpannedResult<Bindings> {
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
            Spanned::new(merge_expr, adjusted_span(map.span, self.current_span_offset)),
        ))
    }

    pub(super) fn map_entry(&mut self, entry: &ast::MapEntry) -> error::SpannedResult<Bindings> {
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
            Spanned::new(entry_expr, adjusted_span(entry.span, self.current_span_offset)),
        ))
    }

    pub(super) fn analyze_string(
        &mut self,
        analyze_string: &ast::AnalyzeString,
    ) -> error::SpannedResult<Bindings> {
        if analyze_string.matching_substring.is_none()
            && analyze_string.non_matching_substring.is_none()
        {
            return Err(error::Error::XTSE1130
                .with_ast_span(adjusted_span(analyze_string.span, self.current_span_offset)));
        }

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
                adjusted_span(analyze_string.span, self.current_span_offset),
            );
            (empty, Bindings::empty())
        };

        // Compile matching-substring body as a closure(xs:string) -> item()*
        let (match_fn_atom, match_fn_bindings) =
            if let Some(matching) = &analyze_string.matching_substring {
                self.analyze_string_closure(&matching.sequence_constructor, analyze_string.span)?
            } else {
                self.empty_closure()?
            };

        // Compile non-matching-substring body as a closure(xs:string) -> item()*
        let (non_match_fn_atom, non_match_fn_bindings) =
            if let Some(non_matching) = &analyze_string.non_matching_substring {
                self.analyze_string_closure(&non_matching.sequence_constructor, analyze_string.span)?
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
                adjusted_span(analyze_string.span, self.current_span_offset),
            ),
        ))
    }

    /// Create a closure that takes a string parameter and evaluates the body
    /// with that string as the context item.
    pub(super) fn analyze_string_closure(
        &mut self,
        sequence_constructor: &[ast::SequenceConstructorItem],
        _span: ast::Span,
    ) -> error::SpannedResult<(ir::AtomS, Bindings)> {
        let context_names = self.variables.push_context();
        let body_bindings = self.with_template_continuation_availability(false, |this| {
            this.sequence_constructor(sequence_constructor)
        })?;
        self.variables.pop_context();
        Ok(self.closure(
            Self::context_params(&context_names),
            body_bindings,
        ))
    }

    /// Create an empty closure that returns the empty sequence.
    pub(super) fn empty_closure(&mut self) -> error::SpannedResult<(ir::AtomS, Bindings)> {
        let item_param = self.variables.new_name();
        let position_param = self.variables.new_name();
        let last_param = self.variables.new_name();
        let empty = self.empty_sequence();
        let body = Bindings::new(self.variables.new_binding_no_span(empty.value));
        Ok(self.closure(
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
            ],
            body,
        ))
    }

    pub(super) fn where_populated(
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
                adjusted_span(where_populated.span, self.current_span_offset),
            ),
        ))
    }

    pub(super) fn on_empty_content(&mut self, on_empty: &ast::OnEmpty) -> error::SpannedResult<Bindings> {
        if let Some(select) = &on_empty.select {
            self.expression(select)
        } else {
            self.sequence_constructor(&on_empty.sequence_constructor)
        }
    }

    pub(super) fn on_non_empty_content(
        &mut self,
        on_non_empty: &ast::OnNonEmpty,
    ) -> error::SpannedResult<Bindings> {
        if let Some(select) = &on_non_empty.select {
            self.expression(select)
        } else {
            self.sequence_constructor(&on_non_empty.sequence_constructor)
        }
    }

    pub(super) fn evaluate(&mut self, evaluate: &ast::Evaluate) -> error::SpannedResult<Bindings> {
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
                        adjusted_span(evaluate.span, self.current_span_offset),
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
            adjusted_span(evaluate.span, self.current_span_offset),
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
            adjusted_span(evaluate.span, self.current_span_offset),
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
                Spanned::new(expr, adjusted_span(evaluate.span, self.current_span_offset)),
            );

        self.convert_bindings(bindings, evaluate.as_.as_ref(), RaisedError::XPTY0004, evaluate.span.clone())
    }

    pub(super) fn evaluate_with_params_argument(
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
            let (key_atom, key_bindings) = self.xml_name(&with_param.name, with_param.span)?.atom_bindings();

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

    pub(super) fn optional_evaluate_base_uri_argument(
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

    pub(super) fn optional_evaluate_argument(
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

    pub(super) fn number(&mut self, number: &ast::Number) -> error::SpannedResult<Bindings> {
        // letter_value is accepted but ignored (implementation-defined)
        // lang and ordinal are compiled and passed to runtime
        // grouping_separator, grouping_size, start_at are compiled and passed to runtime

        // Compile the format AVT (shared between value and counting forms)
        let (format_atom, format_bindings) = self.number_format(&number.format, number.span)?;

        // Compile optional grouping/start-at AVTs
        let (gs_atom, gs_bindings) =
            self.number_optional_avt(&number.grouping_separator, number.span)?;
        let (gsz_atom, gsz_bindings) =
            self.number_optional_avt(&number.grouping_size, number.span)?;
        let (sa_atom, sa_bindings) =
            self.number_optional_avt(&number.start_at, number.span)?;
        let (lang_atom, lang_bindings) =
            self.number_optional_avt(&number.lang, number.span)?;
        let (ordinal_atom, ordinal_bindings) =
            self.number_optional_avt(&number.ordinal, number.span)?;
        let extra_bindings = gs_bindings.concat(gsz_bindings).concat(sa_bindings).concat(lang_bindings).concat(ordinal_bindings);

        if let Some(value) = &number.value {
            // value= form: evaluate expression, format, emit text
            let (value_atom, value_bindings) = self.expression(value)?.atom_bindings();
            let string_expr = self.static_function_call_expr(
                "xslt-number-value",
                FN_NAMESPACE,
                7,
                vec![value_atom, format_atom, gs_atom.clone(), gsz_atom.clone(), sa_atom.clone(), lang_atom.clone(), ordinal_atom.clone()],
            );
            let (text_atom, bindings) = value_bindings
                .concat(format_bindings)
                .concat(extra_bindings)
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
            // Wrap in validation to produce XTTE0990/XTTE1000 instead of XPTY0004
            let (node_atom, node_bindings) = if let Some(select) = &number.select {
                let (raw_atom, raw_bindings) = self.expression(select)?.atom_bindings();
                let validate_expr = self.static_function_call_expr(
                    "xslt-number-validate-select",
                    FN_NAMESPACE,
                    1,
                    vec![raw_atom],
                );
                raw_bindings
                    .bind_expr_no_span(&mut self.variables, validate_expr)
                    .atom_bindings()
            } else {
                match self.variables.context_item(adjusted_span(number.span, self.current_span_offset)) {
                    Ok(bindings) => {
                        let (raw_atom, raw_bindings) = bindings.atom_bindings();
                        let validate_expr = self.static_function_call_expr(
                            "xslt-number-validate-context",
                            FN_NAMESPACE,
                            1,
                            vec![raw_atom],
                        );
                        raw_bindings
                            .bind_expr_no_span(&mut self.variables, validate_expr)
                            .atom_bindings()
                    }
                    Err(_) => {
                        // Context is absent (e.g. inside xsl:function or xsl:on-completion).
                        // Produce XTTE0990 at runtime.
                        let empty_expr = self.empty_sequence();
                        let (raw_atom, raw_bindings) = Bindings::empty()
                            .bind_expr_no_span(&mut self.variables, empty_expr.value)
                            .atom_bindings();
                        let validate_expr = self.static_function_call_expr(
                            "xslt-number-validate-context",
                            FN_NAMESPACE,
                            1,
                            vec![raw_atom],
                        );
                        raw_bindings
                            .bind_expr_no_span(&mut self.variables, validate_expr)
                            .atom_bindings()
                    }
                }
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
                                    adjusted_span(number.span, self.current_span_offset),
                                )),
                            )
                            .atom_bindings();
                        let (from_index_atom, from_index_bindings) = Bindings::empty()
                            .bind_expr_no_span(
                                &mut self.variables,
                                ir::Expr::Atom(Spanned::new(
                                    ir::Atom::Const(ir::Const::Integer(from_index.into())),
                                    adjusted_span(number.span, self.current_span_offset),
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
                            9,
                            vec![node_atom, count_index_atom, from_index_atom, format_atom, gs_atom.clone(), gsz_atom.clone(), sa_atom.clone(), lang_atom.clone(), ordinal_atom.clone()],
                        );
                        let (text_atom, bindings) = node_bindings
                            .concat(format_bindings)
                            .concat(count_index_bindings)
                            .concat(from_index_bindings)
                            .concat(extra_bindings)
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
                            7,
                            vec![node_atom, format_atom, gs_atom.clone(), gsz_atom.clone(), sa_atom.clone(), lang_atom.clone(), ordinal_atom.clone()],
                        );
                        let (text_atom, bindings) = node_bindings
                            .concat(format_bindings)
                            .concat(extra_bindings)
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
                                    adjusted_span(number.span, self.current_span_offset),
                                )),
                            )
                            .atom_bindings();
                        let (from_index_atom, from_index_bindings) = Bindings::empty()
                            .bind_expr_no_span(
                                &mut self.variables,
                                ir::Expr::Atom(Spanned::new(
                                    ir::Atom::Const(ir::Const::Integer(from_index.into())),
                                    adjusted_span(number.span, self.current_span_offset),
                                )),
                            )
                            .atom_bindings();

                        let string_expr = self.static_function_call_expr(
                            "xslt-number-count-multiple-pattern",
                            FN_NAMESPACE,
                            9,
                            vec![node_atom, count_index_atom, from_index_atom, format_atom, gs_atom.clone(), gsz_atom.clone(), sa_atom.clone(), lang_atom.clone(), ordinal_atom.clone()],
                        );
                        let (text_atom, bindings) = node_bindings
                            .concat(format_bindings)
                            .concat(count_index_bindings)
                            .concat(from_index_bindings)
                            .concat(extra_bindings)
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
                            7,
                            vec![node_atom, format_atom, gs_atom.clone(), gsz_atom.clone(), sa_atom.clone(), lang_atom.clone(), ordinal_atom.clone()],
                        );
                        let (text_atom, bindings) = node_bindings
                            .concat(format_bindings)
                            .concat(extra_bindings)
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

    pub(super) fn number_format(
        &mut self,
        format: &Option<ast::ValueTemplate<String>>,
        span: ast::Span,
    ) -> error::SpannedResult<(Spanned<ir::Atom>, Bindings)> {
        if let Some(format) = format {
            Ok(self.attribute_value_template(format)?.atom_bindings())
        } else {
            let bindings = Bindings::empty();
            let format_expr = ir::Expr::Atom(Spanned::new(
                ir::Atom::Const(ir::Const::String("1".to_string())),
                adjusted_span(span, self.current_span_offset),
            ));
            Ok(bindings
                .bind_expr_no_span(&mut self.variables, format_expr)
                .atom_bindings())
        }
    }

    /// Compile an optional AVT attribute for xsl:number.
    /// If present, compiles the AVT to a string. If absent, produces an empty sequence
    /// (which the runtime receives as `None`).
    pub(super) fn number_optional_avt<V>(
        &mut self,
        avt: &Option<ast::ValueTemplate<V>>,
        span: ast::Span,
    ) -> error::SpannedResult<(Spanned<ir::Atom>, Bindings)>
    where
        V: Clone + PartialEq + Eq,
    {
        if let Some(avt) = avt {
            Ok(self.attribute_value_template(avt)?.atom_bindings())
        } else {
            let bindings = Bindings::empty();
            let expr = ir::Expr::Atom(Spanned::new(
                ir::Atom::Const(ir::Const::EmptySequence),
                adjusted_span(span, self.current_span_offset),
            ));
            Ok(bindings
                .bind_expr_no_span(&mut self.variables, expr)
                .atom_bindings())
        }
    }

    pub(super) fn message(&mut self, message: &ast::Message) -> error::SpannedResult<Bindings> {
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
            let (message_atom, message_print_bindings) = message_bindings.atom_bindings();

            // Determine error code (custom or default XTMM9000)
            let (local_name, namespace, prefix) =
                if let Some(error_code) = &message.error_code {
                    if let Some(code_str) = self.static_value_template(error_code) {
                        self.resolve_eqname_string(&code_str, &message.namespaces)
                    } else {
                        None
                    }
                } else {
                    None
                }
                .unwrap_or_else(|| {
                    (
                        "XTMM9000".to_string(),
                        "http://www.w3.org/2005/xqt-errors".to_string(),
                        "err".to_string(),
                    )
                });

            let ns_atom =
                Spanned::new(ir::Atom::Const(ir::Const::String(namespace)), adjusted_span(message.span, self.current_span_offset));
            let local_atom = Spanned::new(
                ir::Atom::Const(ir::Const::String(local_name)),
                adjusted_span(message.span, self.current_span_offset),
            );
            let prefix_atom =
                Spanned::new(ir::Atom::Const(ir::Const::String(prefix)), adjusted_span(message.span, self.current_span_offset));
            let call_expr = self.static_function_call_expr(
                "xslt-message-terminate",
                FN_NAMESPACE,
                4,
                vec![message_atom, ns_atom, local_atom, prefix_atom],
            );
            let error_bindings =
                Bindings::empty().bind_expr_no_span(&mut self.variables, call_expr);
            Ok(message_print_bindings.concat(error_bindings))
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
    pub(super) fn resolve_eqname_string(
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

    pub(super) fn result_document(
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
                    span: Some(adjusted_span(result_document.span, self.current_span_offset).into()),
                    detail: None,

                    contexts: Vec::new(),
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
                Spanned::new(ir::Atom::Const(ir::Const::EmptySequence), adjusted_span(result_document.span, self.current_span_offset)),
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
                        adjusted_span(result_document.span, self.current_span_offset),
                    ),
                    Bindings::empty(),
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String(String::new())),
                        adjusted_span(result_document.span, self.current_span_offset),
                    ),
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String(String::new())),
                        adjusted_span(result_document.span, self.current_span_offset),
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
                            adjusted_span(result_document.span, self.current_span_offset),
                        ),
                        Spanned::new(
                            ir::Atom::Const(ir::Const::String(self.encode_named_outputs()?)),
                            adjusted_span(result_document.span, self.current_span_offset),
                        ),
                    )
                }
                None => (
                    None,
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String(String::new())),
                        adjusted_span(result_document.span, self.current_span_offset),
                    ),
                    Bindings::empty(),
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String(String::new())),
                        adjusted_span(result_document.span, self.current_span_offset),
                    ),
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String(String::new())),
                        adjusted_span(result_document.span, self.current_span_offset),
                    ),
                ),
            };

        let (method_atom, method_bindings) = if let Some(method) = &result_document.method {
            if let Some(method_literal) = self.static_value_template(method) {
                (
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String(method_literal)),
                        adjusted_span(result_document.span, self.current_span_offset),
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
                    adjusted_span(result_document.span, self.current_span_offset),
                ),
                Bindings::empty(),
            )
        } else {
            (
                Spanned::new(
                    ir::Atom::Const(ir::Const::String(String::new())),
                    adjusted_span(result_document.span, self.current_span_offset),
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
                        adjusted_span(result_document.span, self.current_span_offset),
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
                            atom: Spanned::new(self.concat_atom(2), adjusted_span(result_document.span, self.current_span_offset)),
                            args: vec![
                                Spanned::new(
                                    ir::Atom::Const(ir::Const::String(format!(
                                        "{static_cdata} "
                                    ))),
                                    adjusted_span(result_document.span, self.current_span_offset),
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
                    adjusted_span(result_document.span, self.current_span_offset),
                ),
                Bindings::empty(),
            )
        } else {
            (
                Spanned::new(
                    ir::Atom::Const(ir::Const::String(String::new())),
                    adjusted_span(result_document.span, self.current_span_offset),
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
                        adjusted_span(result_document.span, self.current_span_offset),
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
                    adjusted_span(result_document.span, self.current_span_offset),
                ),
                Bindings::empty(),
            )
        } else {
            (
                Spanned::new(
                    ir::Atom::Const(ir::Const::String(String::new())),
                    adjusted_span(result_document.span, self.current_span_offset),
                ),
                Bindings::empty(),
            )
        };

        let (omit_xml_declaration_atom, omit_xml_declaration_bindings) =
            self.validated_value_template_or_literal_atom(
                result_document.omit_xml_declaration.as_ref(),
                formatted_output
                    .as_ref()
                    .map(|output| output.omit_xml_declaration.map(|b| b.to_string()).unwrap_or_default())
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
                        span: Some(adjusted_span(result_document.span, self.current_span_offset).into()),
                        detail: None,

                        contexts: Vec::new(),
                    }
                })?;
                (
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String(html_version_literal)),
                        adjusted_span(result_document.span, self.current_span_offset),
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
                    adjusted_span(result_document.span, self.current_span_offset),
                ),
                Bindings::empty(),
            )
        } else {
            (
                Spanned::new(
                    ir::Atom::Const(ir::Const::String(String::new())),
                    adjusted_span(result_document.span, self.current_span_offset),
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
            adjusted_span(result_document.span, self.current_span_offset),
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
                    adjusted_span(result_document.span, self.current_span_offset),
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
                    adjusted_span(result_document.span, self.current_span_offset),
                ),
            ))
    }

    pub(super) fn sequence_constructor_content(
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

    pub(super) fn sequence_constructor_content_element(
        &mut self,
        element_node: &ast::ElementNode,
    ) -> error::SpannedResult<Bindings> {
        let element_name = self.apply_namespace_alias(&element_node.name);
        let aliased_attributes = element_node
            .attributes
            .iter()
            .map(|(name, value)| (self.apply_namespace_alias(name), value.clone()))
            .collect::<Vec<_>>();

        let (name_atom, bindings) = self.xml_name(&element_name, element_node.span)?.atom_bindings();
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
                adjusted_span(element_node.span, self.current_span_offset),
            );
            let namespace_atom = Spanned::new(
                ir::Atom::Const(ir::Const::String(namespace.uri.clone())),
                adjusted_span(element_node.span, self.current_span_offset),
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
            let (attribute_name_atom, attribute_bindings) = self.xml_name(name, element_node.span)?.atom_bindings();
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

    pub(super) fn attribute_set_append(
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

    pub(super) fn attribute_set_bindings(
        &mut self,
        use_attribute_sets: &[ast::EqName],
    ) -> error::SpannedResult<Bindings> {
        self.attribute_set_bindings_with_active(use_attribute_sets)
    }

    pub(super) fn attribute_set_bindings_with_active(
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

    pub(super) fn comma_bindings(&mut self, left: Bindings, right: Bindings) -> Bindings {
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

    pub(super) fn ensure_name_namespace(namespaces: &mut Vec<ast::LiteralNamespace>, name: &ast::Name) {
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

    pub(super) fn sequence_constructor_append(
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

    pub(super) fn space_separator_atom(&self) -> ir::AtomS {
        Spanned::new(
            ir::Atom::Const(ir::Const::String(" ".to_string())),
            (0..0).into(),
        )
    }

    pub(super) fn apply_templates(
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

        let span = adjusted_span(apply_templates.span, self.current_span_offset);
        Ok(bindings.bind_expr_spanned(
            &mut self.variables,
            ir::Expr::ApplyTemplates(ir::ApplyTemplates {
                mode,
                select: select_atom,
                builtin_template_params_passthrough: apply_templates
                    .builtin_template_params_passthrough,
                params,
            }),
            span,
        ))
    }

    pub(super) fn apply_imports(
        &mut self,
        apply_imports: &ast::ApplyImports,
    ) -> error::SpannedResult<Bindings> {
        self.continue_template(
            apply_imports.with_params.iter(),
            ir::ContinueBehavior::ApplyImports,
        )
    }

    pub(super) fn next_match(&mut self, next_match: &ast::NextMatch) -> error::SpannedResult<Bindings> {
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

    pub(super) fn continue_template<'b>(
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

    pub(super) fn apply_template_sorts(
        &mut self,
        select_atom: ir::AtomS,
        sorts: &[&ast::Sort],
    ) -> error::SpannedResult<(ir::AtomS, Bindings)> {
        let mut current_atom = select_atom;
        let mut bindings = Bindings::empty();

        for sort in sorts.iter().rev() {
            self.ensure_supported_sort(sort)?;
            let (collation_atom, collation_bindings) = self.sort_collation_atom(sort)?;
            let is_descending = self.sort_is_descending(sort)?;

            bindings = bindings.concat(collation_bindings);
            let sort_expr = if self.sort_uses_default_text_key(sort)? {
                // No key function — use fn:sort#2 or xslt-sort-descending#2
                let sort_fn_name = if is_descending {
                    "xslt-sort-descending"
                } else {
                    "sort"
                };
                self.static_function_call_expr(
                    sort_fn_name,
                    FN_NAMESPACE,
                    2,
                    vec![current_atom.clone(), collation_atom],
                )
            } else {
                // Key function with 3 params (item, position, last) —
                // use xslt-sort#3 or xslt-sort-descending#3 which pass
                // context position/size to the key function.
                let sort_fn_name = if is_descending {
                    "xslt-sort-descending"
                } else {
                    "xslt-sort"
                };
                let (key_atom, key_bindings) = self.sort_key_function(sort)?;
                bindings = bindings.concat(key_bindings);
                self.static_function_call_expr(
                    sort_fn_name,
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
        }

        Ok((current_atom, bindings))
    }

    pub(super) fn sort_uses_default_text_key(&self, sort: &ast::Sort) -> error::SpannedResult<bool> {
        Ok(sort.select.is_none()
            && sort.sequence_constructor.is_empty()
            && matches!(self.sort_data_type(sort)?, SortDataType::Text))
    }

    pub(super) fn sort_key_function(
        &mut self,
        sort: &ast::Sort,
    ) -> error::SpannedResult<(ir::AtomS, Bindings)> {
        let item_param = self.variables.new_name();
        let position_param = self.variables.new_name();
        let last_param = self.variables.new_name();
        let context_names = self.variables.push_context();
        let return_bindings = self.sort_key_return_bindings(sort)?;
        self.variables.pop_context();

        let span = adjusted_span(sort.span, self.current_span_offset);

        // Build nested Let bindings: let $item := $item_param in
        //   let $position := $position_param in
        //     let $last := $last_param in <key_expr>
        // This gives the key expression proper context position/size
        // instead of wrapping in a Map over a single item (which always
        // sets position=1, last=1).
        let body = ir::Expr::Let(ir::Let {
            name: context_names.item.clone(),
            var_expr: Box::new(Spanned::new(
                ir::Expr::Atom(Spanned::new(ir::Atom::Variable(item_param.clone()), span)),
                span,
            )),
            return_expr: Box::new(Spanned::new(
                ir::Expr::Let(ir::Let {
                    name: context_names.position.clone(),
                    var_expr: Box::new(Spanned::new(
                        ir::Expr::Atom(Spanned::new(ir::Atom::Variable(position_param.clone()), span)),
                        span,
                    )),
                    return_expr: Box::new(Spanned::new(
                        ir::Expr::Let(ir::Let {
                            name: context_names.last.clone(),
                            var_expr: Box::new(Spanned::new(
                                ir::Expr::Atom(Spanned::new(ir::Atom::Variable(last_param.clone()), span)),
                                span,
                            )),
                            return_expr: Box::new(return_bindings.expr()),
                        }),
                        span,
                    )),
                }),
                span,
            )),
        });
        let function = ir::Expr::FunctionDefinition(ir::FunctionDefinition {
            declared_name: None,
            params: vec![
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
            ],
            return_type: None,
            body: Box::new(Spanned::new(body, span)),
            static_base_uri: self.current_static_base_uri_string(),
        });

        let bindings = Bindings::empty().bind_expr_no_span(&mut self.variables, function);
        Ok(bindings.atom_bindings())
    }

    pub(super) fn sort_key_return_bindings(&mut self, sort: &ast::Sort) -> error::SpannedResult<Bindings> {
        let key_bindings = if let Some(select) = &sort.select {
            self.expression(select)?
        } else if !sort.sequence_constructor.is_empty() {
            self.sequence_constructor_with_temporary_output_state(&sort.sequence_constructor)?
        } else {
            self.variables.context_item(adjusted_span(sort.span, self.current_span_offset))?
        };
        self.atomized_key_bindings(key_bindings, self.sort_data_type(sort)?)
    }

    pub(super) fn atomized_key_bindings(
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

    pub(super) fn sort_collation_atom(
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
                    Spanned::new(ir::Atom::Const(ir::Const::String(uri)), adjusted_span(sort.span, self.current_span_offset)),
                    Bindings::empty(),
                ))
            } else {
                Ok(self.attribute_value_template(collation)?.atom_bindings())
            }
        } else if let Some(suffix) = &case_first_suffix {
            let uri = format!("http://www.w3.org/2013/collation/UCA?{}", suffix);
            Ok((
                Spanned::new(ir::Atom::Const(ir::Const::String(uri)), adjusted_span(sort.span, self.current_span_offset)),
                Bindings::empty(),
            ))
        } else {
            Ok((
                Spanned::new(ir::Atom::Const(ir::Const::EmptySequence), adjusted_span(sort.span, self.current_span_offset)),
                Bindings::empty(),
            ))
        }
    }

    pub(super) fn sort_case_order_suffix(&self, sort: &ast::Sort) -> error::SpannedResult<Option<String>> {
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

    pub(super) fn ensure_supported_sort(&self, _sort: &ast::Sort) -> error::SpannedResult<()> {
        // lang attribute is accepted but silently ignored (uses default Unicode collation)
        Ok(())
    }

    pub(super) fn sort_is_descending(&self, sort: &ast::Sort) -> error::SpannedResult<bool> {
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

    pub(super) fn sort_data_type(&self, sort: &ast::Sort) -> error::SpannedResult<SortDataType> {
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

    pub(super) fn literal_value_template<V>(
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

    pub(super) fn call_template(
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

        let span = adjusted_span(call_template.span, self.current_span_offset);
        Ok(param_bindings.bind_expr_spanned(&mut self.variables, call_template_expr, span))
    }

    pub(super) fn zero_arg_closure(&mut self, body: Bindings) -> (ir::AtomS, Bindings) {
        self.closure(Vec::new(), body)
    }

    pub(super) fn closure(&mut self, params: Vec<ir::Param>, body: Bindings) -> (ir::AtomS, Bindings) {
        let function_definition = ir::FunctionDefinition {
            declared_name: None,
            params,
            return_type: None,
            body: Box::new(body.expr()),
            static_base_uri: self.current_static_base_uri_string(),
        };
        let function_expr = Bindings::empty().bind_expr_no_span(
            &mut self.variables,
            ir::Expr::FunctionDefinition(function_definition),
        );
        function_expr.atom_bindings()
    }

    pub(super) fn catch_error_params() -> Vec<ir::Param> {
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

    pub(super) fn catch_error_variable_names() -> Vec<Name> {
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

    pub(super) fn catch_handler_closure(
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

    pub(super) fn square_array_atom(&mut self, atoms: Vec<ir::AtomS>) -> (ir::AtomS, Bindings) {
        let array_expr = Bindings::empty().bind_expr_no_span(
            &mut self.variables,
            ir::Expr::ArrayConstructor(ir::ArrayConstructor::Square(atoms)),
        );
        array_expr.atom_bindings()
    }

    pub(super) fn try_fallback_bindings(&mut self, try_: &ast::Try) -> error::SpannedResult<Bindings> {
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

    pub(super) fn try_has_prior_output_before_source_document(&self, try_: &ast::Try) -> bool {
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

    pub(super) fn try_(&mut self, try_: &ast::Try) -> error::SpannedResult<Bindings> {
        let processor_xslt_version = self.static_context.processor_xslt_version().unwrap_or(3);
        if processor_xslt_version < 3 {
            if try_.xslt_version > processor_xslt_version {
                return self.try_fallback_bindings(try_);
            }
            return Err(
                error::Error::XTSE0010.with_ast_span(adjusted_span(try_.span, self.current_span_offset))
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
                adjusted_span(try_.span, self.current_span_offset),
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
            adjusted_span(try_.span, self.current_span_offset),
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
            adjusted_span(try_.span, self.current_span_offset),
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

    pub(super) fn select_or_sequence_constructor(
        &mut self,
        instruction: &impl ast::SelectOrSequenceConstructor,
    ) -> error::SpannedResult<Bindings> {
        if let Some(select) = instruction.select() {
            self.expression(select)
        } else {
            self.sequence_constructor(instruction.sequence_constructor())
        }
    }

    pub(super) fn select_or_sequence_constructor_simple_content(
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

    pub(super) fn select_or_sequence_constructor_simple_content_with_separator(
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

    pub(super) fn value_of(&mut self, value_of: &ast::ValueOf) -> error::SpannedResult<Bindings> {
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

    pub(super) fn attribute_value_template<V>(
        &mut self,
        value_template: &ast::ValueTemplate<V>,
    ) -> error::SpannedResult<Bindings>
    where
        V: Clone + PartialEq + Eq,
    {
        let mut all_bindings = Vec::new();
        for item in &value_template.template {
            let bindings = match item {
                ast::ValueTemplateItem::String { text, span } => {
                    let text_atom = Spanned::new(
                        ir::Atom::Const(ir::Const::String(text.clone())),
                        adjusted_span(*span, self.current_span_offset),
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
                ast::ValueTemplateItem::Value { xpath, span } => {
                    let ir_span = adjusted_span(*span, self.current_span_offset);
                    let (atom, bindings) = self.xpath(&xpath.0, &[])?.atom_bindings();
                    let expr = self.simple_content_expr_spanned(atom, self.space_separator_atom(), ir_span);
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

    pub(super) fn variable(
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
                self.convert_bindings(var_bindings, variable.as_.as_ref(), RaisedError::XTTE0570, variable.span.clone())?;
            let name = self.variables.declare_var_name(&variable.name);
            Ok(Some((name, var_bindings)))
        } else {
            Ok(None)
        }
    }

    pub(super) fn temporary_tree(
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

    pub(super) fn empty_sequence(&mut self) -> ir::ExprS {
        Spanned::new(
            ir::Expr::Atom(Spanned::new(
                ir::Atom::Const(ir::Const::EmptySequence),
                (0..0).into(),
            )),
            (0..0).into(),
        )
    }

    pub(super) fn empty_string(&self) -> ir::AtomS {
        Spanned::new(
            ir::Atom::Const(ir::Const::String("".to_string())),
            (0..0).into(),
        )
    }

    pub(super) fn if_(&mut self, if_: &ast::If) -> error::SpannedResult<Bindings> {
        let (condition, bindings) = self.expression(&if_.test)?.atom_bindings();
        let expr = ir::Expr::If(ir::If {
            condition,
            then: Box::new(self.sequence_constructor(&if_.sequence_constructor)?.expr()),
            else_: Box::new(self.empty_sequence()),
        });
        Ok(bindings.bind_expr_no_span(&mut self.variables, expr))
    }

    pub(super) fn choose(&mut self, choose: &ast::Choose) -> error::SpannedResult<Bindings> {
        self.choose_when_otherwise(&choose.when, choose.otherwise.as_ref())
    }

    pub(super) fn choose_when_otherwise(
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

    pub(super) fn for_each(&mut self, for_each: &ast::ForEach) -> error::SpannedResult<Bindings> {
        let (select_atom, bindings) = self.expression(&for_each.select)?.atom_bindings();
        let sort_refs = for_each.sort.iter().collect::<Vec<_>>();

        // XSLT 3.0 §13.1: "If the result is a sequence of nodes, then the
        // population is the same sequence of nodes in document order."
        // Wrap in Deduplicate (which sorts into document order) so that
        // position() inside sort key expressions reflects document order.
        let (select_atom, bindings) = if !sort_refs.is_empty() {
            let span = adjusted_span(for_each.span, self.current_span_offset);
            let dedup_expr = ir::Expr::Deduplicate(Box::new(Spanned::new(
                ir::Expr::Atom(select_atom),
                span,
            )));
            let dedup_bindings =
                Bindings::empty().bind_expr_no_span(&mut self.variables, dedup_expr);
            let (dedup_atom, dedup_bindings) = dedup_bindings.atom_bindings();
            (dedup_atom, bindings.concat(dedup_bindings))
        } else {
            (select_atom, bindings)
        };

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

    pub(super) fn perform_sort(&mut self, perform_sort: &ast::PerformSort) -> error::SpannedResult<Bindings> {
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

    pub(super) fn merge(&mut self, merge: &ast::Merge) -> error::SpannedResult<Bindings> {
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

    pub(super) fn merge_source_key_function(
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
                self.variables.context_item(adjusted_span(merge_source.span, self.current_span_offset))?
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
            var_atom: Spanned::new(ir::Atom::Variable(param_name.clone()), adjusted_span(merge_source.span, self.current_span_offset)),
            return_expr: Box::new(return_bindings.expr()),
        });
        let function = ir::Expr::FunctionDefinition(ir::FunctionDefinition {
            declared_name: None,
            params: vec![ir::Param {
                name: param_name,
                type_: None,
                default: None,
                required: false,
                original_name: None,
                tunnel: false,
            }],
            return_type: None,
            body: Box::new(Spanned::new(body, adjusted_span(merge_source.span, self.current_span_offset))),
            static_base_uri: self.current_static_base_uri_string(),
        });

        let bindings = Bindings::empty().bind_expr_no_span(&mut self.variables, function);
        Ok(bindings.atom_bindings())
    }

    pub(super) fn for_each_group(
        &mut self,
        for_each_group: &ast::ForEachGroup,
    ) -> error::SpannedResult<Bindings> {
        if for_each_group.composite
            || for_each_group.collation.is_some()
        {
            return Err(error::Error::Unsupported(format!(
                "Instruction not supported: {:?}",
                for_each_group
            ))
            .into());
        }

        enum GroupSelector {
            KeyFunction(ast::Expression, &'static str),
            PatternFunction(ast::Pattern, &'static str),
        }

        let group_selector = if let Some(group_by) = &for_each_group.group_by {
            GroupSelector::KeyFunction(group_by.clone(), "xslt-for-each-group-by")
        } else if let Some(group_adjacent) = &for_each_group.group_adjacent {
            GroupSelector::KeyFunction(group_adjacent.clone(), "xslt-for-each-group-adjacent")
        } else if let Some(group_starting_with) = &for_each_group.group_starting_with {
            GroupSelector::PatternFunction(
                group_starting_with.clone(),
                "xslt-for-each-group-starting-with",
            )
        } else if let Some(group_ending_with) = &for_each_group.group_ending_with {
            GroupSelector::PatternFunction(
                group_ending_with.clone(),
                "xslt-for-each-group-ending-with",
            )
        } else {
            return Err(error::Error::Unsupported(format!(
                "Instruction not supported: {:?}",
                for_each_group
            ))
            .into());
        };

        let (select_atom, bindings) = self.expression(&for_each_group.select)?.atom_bindings();
        let (runtime_fn_name, selector_atom, selector_bindings) = match group_selector {
            GroupSelector::KeyFunction(group_key_expr, runtime_fn_name) => {
                let (selector_atom, selector_bindings) = self.group_key_function(&group_key_expr)?;
                (runtime_fn_name, selector_atom, selector_bindings)
            }
            GroupSelector::PatternFunction(pattern, runtime_fn_name) => {
                let (selector_atom, selector_bindings) = self.group_pattern_function(&pattern)?;
                (runtime_fn_name, selector_atom, selector_bindings)
            }
        };

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
                    adjusted_span(for_each_group.span, self.current_span_offset),
                )),
                adjusted_span(for_each_group.span, self.current_span_offset),
            )),
            return_expr: Box::new(Spanned::new(
                ir::Expr::Let(ir::Let {
                    name: context_names.position.clone(),
                    var_expr: Box::new(Spanned::new(
                        ir::Expr::Atom(Spanned::new(
                            ir::Atom::Variable(pos_param.clone()),
                            adjusted_span(for_each_group.span, self.current_span_offset),
                        )),
                        adjusted_span(for_each_group.span, self.current_span_offset),
                    )),
                    return_expr: Box::new(Spanned::new(
                        ir::Expr::Let(ir::Let {
                            name: context_names.last.clone(),
                            var_expr: Box::new(Spanned::new(
                                ir::Expr::Atom(Spanned::new(
                                    ir::Atom::Variable(last_param.clone()),
                                    adjusted_span(for_each_group.span, self.current_span_offset),
                                )),
                                adjusted_span(for_each_group.span, self.current_span_offset),
                            )),
                            return_expr: Box::new(body_bindings.expr()),
                        }),
                        adjusted_span(for_each_group.span, self.current_span_offset),
                    )),
                }),
                adjusted_span(for_each_group.span, self.current_span_offset),
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
                let empty = Spanned::new(ir::Atom::Const(ir::Const::EmptySequence), adjusted_span(for_each_group.span, self.current_span_offset));
                let bindings = Bindings::empty();
                (empty, bindings, false, false)
            };

        let sort_descending_atom = Spanned::new(
            ir::Atom::Const(ir::Const::String(
                if sort_descending { "yes" } else { "no" }.to_string(),
            )),
            adjusted_span(for_each_group.span, self.current_span_offset),
        );
        let sort_numeric_atom = Spanned::new(
            ir::Atom::Const(ir::Const::String(
                if sort_numeric { "yes" } else { "no" }.to_string(),
            )),
            adjusted_span(for_each_group.span, self.current_span_offset),
        );

        let expr = self.static_function_call_expr(
            runtime_fn_name,
            FN_NAMESPACE,
            6,
            vec![
                select_atom,
                selector_atom,
                body_atom,
                sort_key_atom,
                sort_descending_atom,
                sort_numeric_atom,
            ],
        );

        Ok(bindings
            .concat(selector_bindings)
            .concat(body_fn_bindings)
            .concat(sort_key_bindings)
            .bind_expr_no_span(&mut self.variables, expr))
    }

    pub(super) fn group_key_function(
        &mut self,
        group_by: &ast::Expression,
    ) -> error::SpannedResult<(ir::AtomS, Bindings)> {
        let item_param = self.variables.new_name();
        let pos_param = self.variables.new_name();
        let last_param = self.variables.new_name();

        let context_names = self.variables.push_context();
        let bindings = self.expression(group_by)?;
        let bindings = self.atomized_key_bindings(bindings, SortDataType::Text)?;
        self.variables.pop_context();

        let span = adjusted_span(group_by.span, self.current_span_offset);

        // Build Let chain: bind context variables from closure params
        // so that position()/last() work correctly in the key expression
        let body = ir::Expr::Let(ir::Let {
            name: context_names.item.clone(),
            var_expr: Box::new(Spanned::new(
                ir::Expr::Atom(Spanned::new(
                    ir::Atom::Variable(item_param.clone()),
                    span,
                )),
                span,
            )),
            return_expr: Box::new(Spanned::new(
                ir::Expr::Let(ir::Let {
                    name: context_names.position.clone(),
                    var_expr: Box::new(Spanned::new(
                        ir::Expr::Atom(Spanned::new(
                            ir::Atom::Variable(pos_param.clone()),
                            span,
                        )),
                        span,
                    )),
                    return_expr: Box::new(Spanned::new(
                        ir::Expr::Let(ir::Let {
                            name: context_names.last.clone(),
                            var_expr: Box::new(Spanned::new(
                                ir::Expr::Atom(Spanned::new(
                                    ir::Atom::Variable(last_param.clone()),
                                    span,
                                )),
                                span,
                            )),
                            return_expr: Box::new(bindings.expr()),
                        }),
                        span,
                    )),
                }),
                span,
            )),
        });

        let function_definition = ir::FunctionDefinition {
            declared_name: None,
            params: vec![
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
            return_type: None,
            body: Box::new(Spanned::new(body, span)),
            static_base_uri: self.current_static_base_uri_string(),
        };

        let function_expr = Bindings::empty().bind_expr_no_span(
            &mut self.variables,
            ir::Expr::FunctionDefinition(function_definition),
        );
        Ok(function_expr.atom_bindings())
    }

    pub(super) fn group_pattern_function(
        &mut self,
        pattern: &ast::Pattern,
    ) -> error::SpannedResult<(ir::AtomS, Bindings)> {
        let expr = self.group_pattern_match_expr(pattern)?;
        let item_param = self.variables.new_name();
        let pos_param = self.variables.new_name();
        let last_param = self.variables.new_name();

        let context_names = self.variables.push_context();
        let bindings = self.xpath(&expr, &[])?;
        self.variables.pop_context();

        let span = adjusted_span(pattern.span, self.current_span_offset);
        let body = ir::Expr::Let(ir::Let {
            name: context_names.item.clone(),
            var_expr: Box::new(Spanned::new(
                ir::Expr::Atom(Spanned::new(
                    ir::Atom::Variable(item_param.clone()),
                    span,
                )),
                span,
            )),
            return_expr: Box::new(Spanned::new(
                ir::Expr::Let(ir::Let {
                    name: context_names.position.clone(),
                    var_expr: Box::new(Spanned::new(
                        ir::Expr::Atom(Spanned::new(
                            ir::Atom::Variable(pos_param.clone()),
                            span,
                        )),
                        span,
                    )),
                    return_expr: Box::new(Spanned::new(
                        ir::Expr::Let(ir::Let {
                            name: context_names.last.clone(),
                            var_expr: Box::new(Spanned::new(
                                ir::Expr::Atom(Spanned::new(
                                    ir::Atom::Variable(last_param.clone()),
                                    span,
                                )),
                                span,
                            )),
                            return_expr: Box::new(bindings.expr()),
                        }),
                        span,
                    )),
                }),
                span,
            )),
        });

        let function_definition = ir::FunctionDefinition {
            declared_name: None,
            params: vec![
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
            return_type: None,
            body: Box::new(Spanned::new(body, span)),
            static_base_uri: self.current_static_base_uri_string(),
        };

        let function_expr = Bindings::empty().bind_expr_no_span(
            &mut self.variables,
            ir::Expr::FunctionDefinition(function_definition),
        );
        Ok(function_expr.atom_bindings())
    }

    pub(super) fn group_pattern_match_expr(
        &self,
        pattern: &ast::Pattern,
    ) -> error::SpannedResult<xpath_ast::ExprS> {
        let xpath_pattern::Pattern::Expr(xpath_pattern::ExprPattern::Path(path)) = &pattern.pattern else {
            return Err(error::Error::Unsupported(format!(
                "Instruction not supported: {:?}",
                pattern
            ))
            .into());
        };
        let xpath_pattern::PathRoot::Relative = path.root else {
            return Err(error::Error::Unsupported(format!(
                "Instruction not supported: {:?}",
                pattern
            ))
            .into());
        };
        if path.steps.len() != 1 {
            return Err(error::Error::Unsupported(format!(
                "Instruction not supported: {:?}",
                pattern
            ))
            .into());
        }
        let xpath_pattern::StepExpr::AxisStep(axis_step) = &path.steps[0] else {
            return Err(error::Error::Unsupported(format!(
                "Instruction not supported: {:?}",
                pattern
            ))
            .into());
        };

        let axis = match axis_step.forward {
            xpath_pattern::ForwardAxis::Child | xpath_pattern::ForwardAxis::Self_ => {
                xpath_ast::Axis::Self_
            }
            xpath_pattern::ForwardAxis::Attribute => xpath_ast::Axis::Attribute,
            _ => {
                return Err(error::Error::Unsupported(format!(
                    "Instruction not supported: {:?}",
                    pattern
                ))
                .into())
            }
        };

        Ok(Spanned::new(
            xpath_ast::Expr(vec![Spanned::new(
                xpath_ast::ExprSingle::Path(xpath_ast::PathExpr {
                    steps: vec![Spanned::new(
                        xpath_ast::StepExpr::AxisStep(xpath_ast::AxisStep {
                            axis,
                            node_test: axis_step.node_test.clone(),
                            predicates: axis_step.predicates.clone(),
                        }),
                        adjusted_span(pattern.span, self.current_span_offset),
                    )],
                }),
                adjusted_span(pattern.span, self.current_span_offset),
            )]),
            adjusted_span(pattern.span, self.current_span_offset),
        ))
    }

    pub(super) fn iterate(&mut self, iterate: &ast::Iterate) -> error::SpannedResult<Bindings> {
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

    pub(super) fn break_(&mut self, break_: &ast::Break) -> error::SpannedResult<Bindings> {
        let loop_name = self
            .variables
            .current_iterate_loop_name()
            .ok_or(error::SpannedError {
                error: error::Error::XTSE3120,
                span: None,
                detail: None,

                contexts: Vec::new(),
            })?;

        let bindings = self.select_or_sequence_constructor(break_)?;
        let expr = ir::Expr::IterateBreak(ir::IterateBreak {
            loop_name,
            return_expr: Box::new(bindings.expr()),
        });
        Ok(bindings.bind_expr_no_span(&mut self.variables, expr))
    }

    pub(super) fn next_iteration(
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

    pub(super) fn copy(&mut self, copy: &ast::Copy) -> error::SpannedResult<Bindings> {
        let span = adjusted_span(copy.span, self.current_span_offset);

        if let Some(select) = &copy.select {
            // xsl:copy with select: CopyShallow enforces cardinality (XTTE3180
            // if >1 items). The sequence constructor body needs the selected
            // item as context, so we use Map over the select result (0 or 1
            // items after the cardinality check).
            let (select_atom, bindings) = self.expression(select)?.atom_bindings();

            // CopyShallow checks cardinality and creates the shallow copy
            let expr = ir::Expr::CopyShallow(ir::CopyShallow {
                select: select_atom.clone(),
            });
            let (copy_atom, bindings) = bindings
                .bind_expr_no_span(&mut self.variables, expr)
                .atom_bindings();

            // Element/document checks on the copy
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

            // Sequence constructor with context established via Map over
            // select_atom (the original, not the copy — spec says "the element
            // being copied" is the context item)
            let context_names = self.variables.push_context();
            let attribute_set_bindings =
                self.attribute_set_bindings(copy.use_attribute_sets.as_deref().unwrap_or(&[]))?;
            let sc_bindings = self.with_template_continuation_availability(false, |this| {
                this.sequence_constructor(&copy.sequence_constructor)
            })?;
            self.variables.pop_context();

            let inner_bindings = if copy.use_attribute_sets.is_some() {
                self.comma_bindings(attribute_set_bindings, sc_bindings)
            } else {
                sc_bindings
            };

            let map_expr = ir::Expr::Map(ir::Map {
                context_names,
                var_atom: select_atom,
                return_expr: Box::new(inner_bindings.expr()),
            });
            let (body_atom, body_bindings) =
                Bindings::empty()
                    .bind_expr_no_span(&mut self.variables, map_expr)
                    .atom_bindings();

            let bindings = bindings.concat(body_bindings);

            let append = ir::Expr::XmlAppend(ir::XmlAppend {
                parent: copy_atom,
                child: body_atom,
            });

            let if_expr = ir::Expr::If(ir::If {
                condition: is_element_or_document_atom,
                then: Box::new(Spanned::new(append, span)),
                else_: Box::new(Spanned::new(copy_expr, span)),
            });

            Ok(bindings.bind_expr_no_span(&mut self.variables, if_expr))
        } else {
            self.copy_body(copy)
        }
    }

    pub(super) fn copy_body(&mut self, copy: &ast::Copy) -> error::SpannedResult<Bindings> {
        let (context_atom, bindings) = self.variables
            .context_item(adjusted_span(copy.span, self.current_span_offset))?
            .atom_bindings();

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

        let sequence_constructor_bindings =
            self.sequence_constructor(&copy.sequence_constructor)?;
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
            then: Box::new(Spanned::new(append, adjusted_span(copy.span, self.current_span_offset))),
            else_: Box::new(Spanned::new(copy_expr, adjusted_span(copy.span, self.current_span_offset))),
        });

        Ok(bindings.bind_expr_no_span(&mut self.variables, if_expr))
    }

    pub(super) fn is_document_expr(&self, atom: ir::AtomS) -> ir::Expr {
        ir::Expr::InstanceOf(ir::InstanceOf {
            atom,
            sequence_type: xpath_ast::SequenceType::Item(xpath_ast::Item {
                item_type: xpath_ast::ItemType::KindTest(xpath_ast::KindTest::Document(None)),
                occurrence: xpath_ast::Occurrence::One,
            }),
        })
    }

    pub(super) fn is_element_expr(&self, atom: ir::AtomS) -> ir::Expr {
        ir::Expr::InstanceOf(ir::InstanceOf {
            atom,
            sequence_type: xpath_ast::SequenceType::Item(xpath_ast::Item {
                item_type: xpath_ast::ItemType::KindTest(xpath_ast::KindTest::Element(None)),
                occurrence: xpath_ast::Occurrence::One,
            }),
        })
    }

    pub(super) fn copy_of(&mut self, copy_of: &ast::CopyOf) -> error::SpannedResult<Bindings> {
        let (atom, bindings) = self.expression(&copy_of.select)?.atom_bindings();
        let copy_deep_expr = ir::Expr::CopyDeep(ir::CopyDeep { select: atom });
        Ok(bindings.bind_expr_no_span(&mut self.variables, copy_deep_expr))
    }

    pub(super) fn sequence(&mut self, sequence: &ast::Sequence) -> error::SpannedResult<Bindings> {
        self.select_or_sequence_constructor(sequence)
    }
}
