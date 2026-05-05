//! XML node construction.
//!
//! Compiles XSLT instructions that produce XML nodes: xsl:element, xsl:attribute,
//! xsl:text, xsl:comment, xsl:processing-instruction, xsl:namespace, xsl:document,
//! and xsl:source-document. Also handles dynamic name computation (AVTs for
//! element/attribute names) and literal namespace encoding.

use xee_name::FN_NAMESPACE;

use std::collections::HashSet;
use xee_interpreter::error;
use xee_ir::{ir, Bindings};
use xee_xpath_ast::span::Spanned;
use xee_xslt_ast::ast;
use xot::xmlname::NameStrInfo;

use super::{adjusted_span, IrConverter};

impl<'a> IrConverter<'a> {
    /// Compile a static element/attribute name into an IR XmlName binding.
    pub(super) fn xml_name(&mut self, name: &ast::Name, span: ast::Span) -> error::SpannedResult<Bindings> {
        let ir_span = adjusted_span(span, self.current_span_offset);
        let local_name = Spanned::new(
            ir::Atom::Const(ir::Const::String(name.local_name().to_string())),
            ir_span,
        );
        let namespace = Spanned::new(
            ir::Atom::Const(ir::Const::String(name.namespace().to_string())),
            ir_span,
        );

        let binding = self
            .variables
            .new_binding_no_span(ir::Expr::XmlName(ir::XmlName {
                local_name,
                namespace,
            }));
        Ok(Bindings::new(binding))
    }

    /// Compile a dynamic element/attribute name (possibly an AVT) into IR.
    /// Handles both the case where the name is fully static and the dynamic case
    /// where namespace resolution happens at runtime.
    pub(super) fn xml_name_dynamic(
        &mut self,
        name: &ast::ValueTemplate<String>,
        namespace: &Option<ast::ValueTemplate<String>>,
        namespaces: &[ast::LiteralNamespace],
        default_namespace: &str,
        span: ast::Span,
    ) -> error::SpannedResult<Bindings> {
        let ir_span = adjusted_span(span, self.current_span_offset);
        let literal_name = self.static_value_template(name);
        let (localname_atom, bindings) = if let Some((local_name, namespace_uri)) =
            literal_name.as_deref().and_then(|literal_name| {
                self.resolve_static_qname_with_default(literal_name, namespaces, default_namespace)
            }) {
            let local_name_atom = Spanned::new(
                ir::Atom::Const(ir::Const::String(local_name)),
                ir_span,
            );
            let bindings = Bindings::empty()
                .bind_expr_no_span(&mut self.variables, ir::Expr::Atom(local_name_atom));
            if namespace.is_none() {
                let namespace_atom = Spanned::new(
                    ir::Atom::Const(ir::Const::String(namespace_uri)),
                    ir_span,
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
                ir_span,
            );
            let namespace_map_atom = Spanned::new(
                ir::Atom::Const(ir::Const::String(
                    self.encode_literal_namespaces(namespaces),
                )),
                ir_span,
            );
            let (force_namespace_atom, namespace_bindings) = if let Some(namespace) = namespace {
                self.attribute_value_template(namespace)?.atom_bindings()
            } else {
                (
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String(String::new())),
                        ir_span,
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

    pub(super) fn ncname_dynamic(
        &mut self,
        name: &ast::ValueTemplate<String>,
    ) -> error::SpannedResult<Bindings> {
        self.attribute_value_template(name)
    }

    pub(super) fn static_value_template<V>(&self, value_template: &ast::ValueTemplate<V>) -> Option<String>
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

    pub(super) fn resolve_static_qname_with_default(
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

    pub(super) fn default_element_namespace(&self, namespaces: &[ast::LiteralNamespace]) -> String {
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

    /// Encode in-scope literal namespace bindings as a hex-encoded string
    /// for passing to the runtime node constructor.
    pub(super) fn encode_literal_namespaces(&self, namespaces: &[ast::LiteralNamespace]) -> String {
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

    pub(super) fn hex_encode(value: &str) -> String {
        value
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect()
    }

    pub(super) fn static_name_namespace(
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

    pub(super) fn element(&mut self, element: &ast::Element) -> error::SpannedResult<Bindings> {
        let default_namespace = self.default_element_namespace(&element.namespaces);
        let (name_atom, bindings) = self
            .xml_name_dynamic(
                &element.name,
                &element.namespace,
                &element.namespaces,
                &default_namespace,
                element.span,
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
                adjusted_span(element.span, self.current_span_offset),
            );
            let namespace_atom = Spanned::new(
                ir::Atom::Const(ir::Const::String(namespace.uri)),
                adjusted_span(element.span, self.current_span_offset),
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

    pub(super) fn document(&mut self, document: &ast::Document) -> error::SpannedResult<Bindings> {
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

    pub(super) fn source_document(
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
                adjusted_span(source_document.span, self.current_span_offset),
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
                adjusted_span(source_document.span, self.current_span_offset),
            ),
        ))
    }

    pub(super) fn text(&mut self, text: &ast::Text) -> error::SpannedResult<Bindings> {
        let (atom, bindings) = self
            .attribute_value_template(&text.content)?
            .atom_bindings();
        Ok(bindings.bind_expr_no_span(
            &mut self.variables,
            ir::Expr::XmlText(ir::XmlText { value: atom }),
        ))
    }

    pub(super) fn attribute(&mut self, attribute: &ast::Attribute) -> error::SpannedResult<Bindings> {
        let (name_atom, name_bindings) = self
            .xml_name_dynamic(
                &attribute.name,
                &attribute.namespace,
                &attribute.namespaces,
                "",
                attribute.span,
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

    pub(super) fn namespace(&mut self, namespace: &ast::Namespace) -> error::SpannedResult<Bindings> {
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

    pub(super) fn comment(&mut self, comment: &ast::Comment) -> error::SpannedResult<Bindings> {
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

    pub(super) fn processing_instruction(
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
    //         atom: Spanned::new(error_atom, adjusted_span(pi.span, self.current_span_offset)),
    //         args: vec![],
    //     });
    //     Ok(Bindings::new(self.variables.new_binding_no_span(expr)))
    // }
}
