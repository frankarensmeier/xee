// Static evaluation of an XSLT stylesheet
// This handles:
// - Whitespace cleanup
// - Static parameters and variables
// - use-when
// - shadow attributes

// The end result is the static global variables, and modified XML tree
// that has any element with use-when that evaluates to false removed,
// as well as any shadow attributes resolved to normal attributes.
// Any attribute on an XSLT element prefixed by _ is taken as a shadow
// attribute - if the attribute later on turns on not to exist, then
// we get a parse error then.

// The procedure is quite tricky: in order to parse xpath expressions
// statically we need to pass in the names of any known global variables that
// we've encountered before.

use std::{cmp::Ordering, collections::HashMap, path::{Path, PathBuf}};

use iri_string::types::{IriAbsoluteString, IriReferenceStr};
use xot::{xmlname::NameStrInfo, NameId, Node, Xot};

use xee_xpath_ast::ast as xpath_ast;
use xee_xpath_compiler::{compile, context::Variables, error, sequence::{Item, Sequence}};
use xee_xpath_compiler::context::StaticContext;
use xee_xpath_ast::FN_NAMESPACE;

use crate::attributes::Attributes;
use crate::content::Content;
use crate::context::Context;
use crate::error::ElementError;
use crate::parse::parse_transform_with_static_variables_and_location;
use crate::state::State;
use crate::whitespace::strip_whitespace;

#[derive(Clone)]
struct StaticVariableEntry {
    value: Sequence,
    precedence: Vec<usize>,
    is_param: bool,
}

fn compare_precedence(left: &[usize], right: &[usize]) -> Ordering {
    for (left_part, right_part) in left.iter().zip(right.iter()) {
        match left_part.cmp(right_part) {
            Ordering::Equal => continue,
            ordering => return ordering,
        }
    }

    if left.len() == right.len() {
        Ordering::Equal
    } else if left.len() < right.len() {
        Ordering::Greater
    } else {
        Ordering::Less
    }
}

fn sequence_contains_function_items(value: &Sequence) -> bool {
    value.iter().any(|item| matches!(item, Item::Function(_)))
}

fn static_values_consistent(
    existing: &StaticVariableEntry,
    value: &Sequence,
    is_param: bool,
) -> bool {
    existing.is_param == is_param
        && !sequence_contains_function_items(&existing.value)
        && !sequence_contains_function_items(value)
        && existing.value == *value
}

fn disable_use_when_restricted_functions(
    static_context: &mut StaticContext,
    xslt_version: u8,
) {
    for local_name in ["current", "key", "unparsed-entity-uri", "unparsed-entity-public-id"] {
        static_context.disable_function(xot::xmlname::OwnedName::new(
            local_name.to_string(),
            FN_NAMESPACE.to_string(),
            String::new(),
        ));
    }

    if xslt_version < 3 {
        static_context.disable_function(xot::xmlname::OwnedName::new(
            "generate-id".to_string(),
            FN_NAMESPACE.to_string(),
            String::new(),
        ));
    }
}

fn restricted_document_access_use_when_result(
    xpath: &xpath_ast::XPath,
    xslt_version: u8,
) -> Option<Sequence> {
    if xslt_version >= 3 {
        return None;
    }

    let exprsingles = &xpath.0.value.0;
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

    let namespace = function_call.name.value.namespace();
    if !namespace.is_empty() && namespace != FN_NAMESPACE {
        return None;
    }

    let restricted = matches!(function_call.name.value.local_name(), "doc-available");
    if !restricted {
        return None;
    }

    Some(Sequence::from(false))
}

struct StaticEvaluator {
    static_global_variables: Variables,
    static_global_variable_entries: HashMap<xpath_ast::Name, StaticVariableEntry>,
    static_parameters: Variables,
    processor_xslt_version: Option<u8>,
    processor_xpath_version: Option<u8>,
    base_dir: Option<PathBuf>,
    document_base_uri: Option<IriAbsoluteString>,
    module_precedence: Vec<usize>,
    honor_document_use_when: bool,
    to_remove: Vec<Node>,
    to_remove_attribute: Vec<(Node, NameId)>,
}

impl StaticEvaluator {
    fn new(
        initial_static_variables: Variables,
        static_parameters: Variables,
        processor_xslt_version: Option<u8>,
        processor_xpath_version: Option<u8>,
        base_dir: Option<PathBuf>,
        module_precedence: Vec<usize>,
        static_base_uri: Option<String>,
        honor_document_use_when: bool,
    ) -> Self {
        let static_global_variable_entries = initial_static_variables
            .iter()
            .map(|(name, value)| {
                (
                    name.clone(),
                    StaticVariableEntry {
                        value: value.clone(),
                        precedence: module_precedence.clone(),
                        is_param: false,
                    },
                )
            })
            .collect();
        Self {
            static_global_variables: initial_static_variables,
            static_global_variable_entries,
            static_parameters,
            processor_xslt_version,
            processor_xpath_version,
            base_dir,
            document_base_uri: static_base_uri.and_then(|uri| uri.try_into().ok()),
            module_precedence,
            honor_document_use_when,
            to_remove: Vec::new(),
            to_remove_attribute: Vec::new(),
        }
    }

    fn path_to_file_uri(path: &Path) -> String {
        format!("file://{}", path.display()).replace(' ', "%20")
    }

    fn remember_static_variable(
        &mut self,
        name: xpath_ast::Name,
        value: Sequence,
        is_param: bool,
        span: crate::ast_core::Span,
    ) -> Result<(), ElementError> {
        if let Some(existing) = self.static_global_variable_entries.get(&name) {
            if compare_precedence(&self.module_precedence, &existing.precedence) == Ordering::Greater
                && !static_values_consistent(existing, &value, is_param)
            {
                return Err(ElementError::XPathRunTime(
                    error::Error::XTSE3450.with_ast_span((span.start..span.end).into()),
                ));
            }
        }

        self.static_global_variables.insert(name.clone(), value.clone());
        self.static_global_variable_entries.insert(
            name,
            StaticVariableEntry {
                value,
                precedence: self.module_precedence.clone(),
                is_param,
            },
        );
        Ok(())
    }

    fn resolve_stylesheet_href(&self, href: &str) -> (PathBuf, Option<PathBuf>) {
        let path = if let Some(base_dir) = &self.base_dir {
            base_dir.join(href)
        } else {
            PathBuf::from(href)
        };
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        let next_base_dir = canonical
            .parent()
            .or_else(|| path.parent())
            .map(std::path::Path::to_path_buf);
        (path, next_base_dir)
    }

    fn merge_imported_static_variable(
        &mut self,
        name: xpath_ast::Name,
        value: Sequence,
        precedence: Vec<usize>,
        is_param: bool,
        span: crate::ast_core::Span,
    ) -> Result<(), ElementError> {
        if let Some(existing) = self.static_global_variable_entries.get(&name) {
            match compare_precedence(&precedence, &existing.precedence) {
                Ordering::Greater => {
                    if !static_values_consistent(existing, &value, is_param) {
                        return Err(ElementError::XPathRunTime(
                            error::Error::XTSE3450.with_ast_span((span.start..span.end).into()),
                        ));
                    }
                }
                Ordering::Equal => {
                    self.static_global_variables.insert(name.clone(), value.clone());
                    self.static_global_variable_entries.insert(
                        name,
                        StaticVariableEntry {
                            value,
                            precedence,
                            is_param,
                        },
                    );
                }
                Ordering::Less => {}
            }
            return Ok(());
        }

        self.static_global_variables.insert(name.clone(), value.clone());
        self.static_global_variable_entries.insert(
            name,
            StaticVariableEntry {
                value,
                precedence,
                is_param,
            },
        );
        Ok(())
    }

    fn evaluate_stylesheet_reference(
        &mut self,
        attributes: &Attributes,
        href: &str,
        precedence: Vec<usize>,
    ) -> Result<(), ElementError> {
        let (path, base_dir) = self.resolve_stylesheet_href(href);
        let resolved_path = path.canonicalize().unwrap_or_else(|_| path.clone());
        let content = std::fs::read_to_string(&path).map_err(|_| {
            ElementError::Unsupported(format!("Could not read stylesheet: {href}"))
        })?;
        let (_, imported_static_variables) = parse_transform_with_static_variables_and_location(
            &content,
            self.static_global_variables.clone(),
            self.processor_xslt_version,
            self.processor_xpath_version,
            base_dir,
            Some(Self::path_to_file_uri(&resolved_path)),
            true,
        )?;
        let span = attributes
            .content
            .state
            .span(attributes.content.node)
            .ok_or(ElementError::Internal)?;

        for (name, value) in imported_static_variables {
            self.merge_imported_static_variable(name, value, precedence.clone(), false, span)?;
        }
        Ok(())
    }

    fn evaluate_top_level_import_or_include(
        &mut self,
        attributes: &Attributes,
        import_index: usize,
    ) -> Result<bool, ElementError> {
        let element = attributes
            .content
            .state
            .xot
            .element(attributes.content.node)
            .ok_or(ElementError::Internal)?;
        let (local_name, namespace) = attributes.content.state.xot.name_ns_str(element.name());
        if namespace != "http://www.w3.org/1999/XSL/Transform" {
            return Ok(false);
        }

        let href_name = attributes.content.state.names.href;
        let Some(href) = attributes.content.state.xot.attributes(attributes.content.node).get(href_name) else {
            return Ok(false);
        };

        match local_name {
            "import" => {
                let mut precedence = self.module_precedence.clone();
                precedence.push(import_index);
                self.evaluate_stylesheet_reference(attributes, href, precedence)?;
                Ok(true)
            }
            "include" => {
                self.evaluate_stylesheet_reference(
                    attributes,
                    href,
                    self.module_precedence.clone(),
                )?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn evaluate_top_level(
        &mut self,
        top_node: Node,
        state: &mut State,
        top_context: Context,
        // this xot is not the same as the one in state, as
        // it's the one used for parameters
        xot: &mut Xot,
    ) -> Result<(), ElementError> {
        let names = &state.names;
        let mut node = state.xot.first_child(top_node);
        let mut import_index = 0usize;

        let top_content = Content::new(top_node, state, top_context);
        let top_attributes = top_content.attributes(state.xot.element(top_node).unwrap());
        let top_attributes = top_attributes.with_standard()?.with_static_standard()?;
        let mut context = top_attributes.content.context.clone();

        while let Some(current) = node {
            if let Some(element) = state.xot.element(current) {
                let current_content = Content::new(current, state, context);
                let attributes = current_content.attributes(element);
                let attributes = attributes.with_standard()?.with_static_standard()?;
                if !self.evaluate_use_when(&attributes, xot)? {
                    self.to_remove.push(current);
                    context = attributes.content.context;
                } else if self.evaluate_top_level_import_or_include(&attributes, import_index)? {
                    let (local_name, namespace) = state.xot.name_ns_str(element.name());
                    if namespace == "http://www.w3.org/1999/XSL/Transform" && local_name == "import" {
                        import_index += 1;
                    }
                    context = self.context_with_static_variables(attributes.content.context.clone());
                    node = state.xot.next_sibling(current);
                    continue;
                } else if element.name() == names.xsl_variable {
                    context = self.evaluate_variable(attributes, xot)?;
                } else if element.name() == names.xsl_param {
                    context = self.evaluate_param(attributes, xot)?;
                } else {
                    context = self.evaluate_other(attributes, xot)?;
                }
            }
            node = state.xot.next_sibling(current);
        }
        Ok(())
    }

    fn context_with_static_variables(&self, mut context: Context) -> Context {
        for name in self.static_global_variables.keys() {
            context = context.with_variable_name(name);
        }
        context
    }

    fn update_tree(&self, state: &mut State) -> Result<(), ElementError> {
        for node in &self.to_remove {
            state
                .xot
                .remove(*node)
                .map_err(|_| ElementError::Internal)?;
        }
        Ok(())
    }

    fn evaluate_variable(
        &mut self,
        attributes: Attributes,
        xot: &mut Xot,
    ) -> Result<Context, ElementError> {
        let names = &attributes.content.state.names;
        let context = if attributes.boolean_with_default(names.static_, false)? {
            let name = attributes.required(names.name, attributes.eqname())?;
            let select = attributes.required(names.select, attributes.xpath())?;
            let value = self.evaluate_static_xpath(select.xpath, &attributes.content, xot)?;
            let context = attributes.content.context.with_variable_name(&name);
            let span = attributes
                .content
                .state
                .span(attributes.content.node)
                .ok_or(ElementError::Internal)?;
            self.remember_static_variable(name, value, false, span)?;
            context
        } else {
            attributes.content.context.clone()
        };
        self.evaluate_children(attributes, xot)?;
        Ok(context)
    }

    fn evaluate_param(
        &mut self,
        attributes: Attributes,
        xot: &mut Xot,
    ) -> Result<Context, ElementError> {
        let names = &attributes.content.state.names;
        let context = if attributes.boolean_with_default(names.static_, false)? {
            let name = attributes.required(names.name, attributes.eqname())?;
            let required = attributes.boolean_with_default(names.required, false)?;
            let context = attributes.content.context.with_variable_name(&name);
            let value = self.static_parameters.get(&name);
            let insert_value = if let Some(value) = value {
                value.clone()
            } else if required {
                // TODO: a required value is mandatory, should return proper error
                return Err(ElementError::Unsupported(String::from(
                    "Required value is mandatory",
                )));
            } else {
                let select = attributes.optional(names.select, attributes.xpath())?;
                if let Some(select) = select {
                    self.evaluate_static_xpath(select.xpath, &attributes.content, xot)?
                } else {
                    // we interpret 'as' as a string here, as we really only want to
                    // check for its existence
                    let as_ = attributes.optional(names.as_, attributes.string())?;
                    if as_.is_some() {
                        Sequence::default()
                    } else {
                        Sequence::from("")
                    }
                }
            };
            let span = attributes
                .content
                .state
                .span(attributes.content.node)
                .ok_or(ElementError::Internal)?;
            self.remember_static_variable(name, insert_value, true, span)?;
            context
        } else {
            attributes.content.context.clone()
        };
        self.evaluate_children(attributes, xot)?;
        Ok(context)
    }

    fn evaluate_other(
        &mut self,
        attributes: Attributes,
        xot: &mut Xot,
    ) -> Result<Context, ElementError> {
        let context = attributes.content.context.clone();
        self.evaluate_node(attributes, xot)?;
        Ok(context)
    }

    fn evaluate_node(&mut self, attributes: Attributes, xot: &mut Xot) -> Result<(), ElementError> {
        let attributes = attributes.with_standard()?.with_static_standard()?;
        if self.evaluate_use_when(&attributes, xot)? {
            self.evaluate_children(attributes, xot)?;
        } else {
            self.to_remove.push(attributes.content.node);
        }
        Ok(())
    }

    fn evaluate_children(
        &mut self,
        attributes: Attributes,
        xot: &mut Xot,
    ) -> Result<(), ElementError> {
        for node in attributes
            .content
            .state
            .xot
            .children(attributes.content.node)
        {
            let content = attributes.content.with_node(node);
            if let Some(element) = content.state.xot.element(node) {
                let attributes = content.attributes(element);
                self.evaluate_node(attributes, xot)?;
            }
        }
        Ok(())
    }

    fn evaluate_use_when(
        &mut self,
        attributes: &Attributes,
        xot: &mut Xot,
    ) -> Result<bool, ElementError> {
        let names = &attributes.content.state.names;
        let use_when = if attributes.in_xsl_namespace() {
            attributes.optional(names.standard.use_when, attributes.xpath())?
        } else {
            attributes.optional(names.xsl_standard.use_when, attributes.xpath())?
        };

        if let Some(use_when) = use_when {
            let value = if let Some(value) = restricted_document_access_use_when_result(
                &use_when.xpath,
                attributes.content.context.xslt_version_major(),
            ) {
                value
            } else {
                self.evaluate_static_xpath(use_when.xpath, &attributes.content, xot)?
            };
            if !value
                .effective_boolean_value()
                // TODO: the way the span is added is ugly, but it ought
                // to at least describe the span of the use-when attribute
                .map_err(|e| e.with_span((use_when.span.start..use_when.span.end).into()))?
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn evaluate_static_xpath(
        &self,
        xpath: xpath_ast::XPath,
        content: &Content,
        xot: &mut Xot,
    ) -> Result<Sequence, xee_xpath_compiler::error::SpannedError> {
        let parser_context = content.parser_context();
        let mut static_context: StaticContext = parser_context.into();
        static_context =
            static_context.clone_with_static_base_uri(self.effective_static_base_uri(content));
        static_context.set_stylesheet_xslt_version(Some(content.context.xslt_version_major()));
        static_context.set_processor_xslt_version(self.processor_xslt_version);
        static_context.set_processor_xpath_version(self.processor_xpath_version);
        disable_use_when_restricted_functions(
            &mut static_context,
            self.processor_xslt_version.unwrap_or(content.context.xslt_version_major()),
        );
        let program = compile(static_context, xpath)?;
        let mut dynamic_context_builder = program.dynamic_context_builder();
        // TODO doing the clone here of the global variables isn't ideal
        dynamic_context_builder.variables(self.static_global_variables.clone());

        let dynamic_context = dynamic_context_builder.build();
        let runnable = program.runnable(&dynamic_context);

        runnable.many(xot)
    }

    fn effective_static_base_uri(&self, content: &Content) -> Option<IriAbsoluteString> {
        self.node_base_uri(content.state, content.node)
    }

    fn node_base_uri(&self, state: &State, node: Node) -> Option<IriAbsoluteString> {
        match state.xot.value(node) {
            xot::Value::Document => self.document_base_uri.clone(),
            xot::Value::Element(_) => {
                if let Some(base) = state.xot.attributes(node).get(state.names.xml_base) {
                    let base: &IriReferenceStr = base.as_str().try_into().ok()?;
                    match base.to_iri() {
                        Ok(iri) => iri.to_owned().try_into().ok(),
                        Err(iri) => {
                            let parent = state.xot.parent(node)?;
                            let parent_base = self.node_base_uri(state, parent)?;
                            iri.resolve_against(&parent_base).to_string().try_into().ok()
                        }
                    }
                } else if let Some(parent) = state.xot.parent(node) {
                    self.node_base_uri(state, parent)
                } else {
                    self.document_base_uri.clone()
                }
            }
            xot::Value::Attribute(_)
            | xot::Value::Comment(_)
            | xot::Value::Text(_)
            | xot::Value::ProcessingInstruction(_) => state
                .xot
                .parent(node)
                .and_then(|parent| self.node_base_uri(state, parent)),
            xot::Value::Namespace(_) => None,
        }
    }
}

pub(crate) fn static_evaluate(
    state: &mut State,
    node: Node,
    static_parameters: Variables,
    xot: &mut Xot,
) -> Result<Variables, ElementError> {
    static_evaluate_with_initial_variables(
        state,
        node,
        Variables::new(),
        static_parameters,
        None,
        None,
        None,
        xot,
    )
}

pub(crate) fn static_evaluate_with_initial_variables(
    state: &mut State,
    node: Node,
    initial_static_variables: Variables,
    static_parameters: Variables,
    processor_xslt_version: Option<u8>,
    processor_xpath_version: Option<u8>,
    base_dir: Option<PathBuf>,
    xot: &mut Xot,
) -> Result<Variables, ElementError> {
    static_evaluate_with_initial_variables_and_location(
        state,
        node,
        initial_static_variables,
        static_parameters,
        processor_xslt_version,
        processor_xpath_version,
        base_dir,
        None,
        false,
        xot,
    )
}

pub(crate) fn static_evaluate_with_initial_variables_and_location(
    state: &mut State,
    node: Node,
    initial_static_variables: Variables,
    static_parameters: Variables,
    processor_xslt_version: Option<u8>,
    processor_xpath_version: Option<u8>,
    base_dir: Option<PathBuf>,
    static_base_uri: Option<String>,
    honor_document_use_when: bool,
    xot: &mut Xot,
) -> Result<Variables, ElementError> {
    strip_whitespace(&mut state.xot, &state.names, node);
    let mut top_context = Context::empty();
    for name in initial_static_variables.keys() {
        top_context = top_context.with_variable_name(name);
    }
    let mut evaluator = StaticEvaluator::new(
        initial_static_variables,
        static_parameters,
        processor_xslt_version,
        processor_xpath_version,
        base_dir,
        Vec::new(),
        static_base_uri,
        honor_document_use_when,
    );

    if evaluator.honor_document_use_when {
        let element = state.xot.element(node).ok_or(ElementError::Internal)?;
        let top_content = Content::new(node, state, top_context.clone());
        let top_attributes = top_content.attributes(element);
        let top_attributes = top_attributes.with_standard()?.with_static_standard()?;
        if !evaluator.evaluate_use_when(&top_attributes, xot)? {
            for child in state.xot.children(node).collect::<Vec<_>>() {
                evaluator.to_remove.push(child);
            }
            evaluator.update_tree(state)?;
            return Ok(evaluator.static_global_variables);
        }
    }

    evaluator.evaluate_top_level(node, state, top_context, xot)?;
    evaluator.update_tree(state)?;

    Ok(evaluator.static_global_variables)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::Names;

    use xee_xpath_compiler::sequence::Item;

    #[test]
    fn test_one_static_variable() {
        let xml = r#"
        <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
            <xsl:variable name="x" static="yes" select="'foo'"/>
        </xsl:stylesheet>
        "#;
        let mut xot = Xot::new();
        let (root, span_info) = xot.parse_with_span_info(xml).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(root).unwrap();

        let mut state = State::new(xot, span_info, names);

        let mut xot = Xot::new();
        let variables =
            static_evaluate(&mut state, document_element, Variables::new(), &mut xot).unwrap();
        assert_eq!(variables.len(), 1);
        let name = xpath_ast::Name::name("x");
        assert_eq!(variables.get(&name), Some(&Item::from("foo").into()));
    }

    #[test]
    fn test_static_variable_depends_on_another() {
        let xml = r#"
        <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
            <xsl:variable name="x" static="yes" select="'foo'"/>
            <xsl:variable name="y" static="yes" select="concat($x, '!')"/>
        </xsl:stylesheet>
        "#;
        let mut xot = Xot::new();
        let (root, span_info) = xot.parse_with_span_info(xml).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(root).unwrap();

        let mut state = State::new(xot, span_info, names);

        let mut xot = Xot::new();
        let variables =
            static_evaluate(&mut state, document_element, Variables::new(), &mut xot).unwrap();
        assert_eq!(variables.len(), 2);
        let name = xpath_ast::Name::name("y");
        assert_eq!(variables.get(&name), Some(&Item::from("foo!").into()));
    }

    #[test]
    fn test_one_parameter_present() {
        let xml = r#"
        <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
            <xsl:param name="x" static="yes" select="'foo'"/>
        </xsl:stylesheet>
        "#;
        let mut xot = Xot::new();
        let (root, span_info) = xot.parse_with_span_info(xml).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(root).unwrap();

        let name = xpath_ast::Name::name("x");
        let static_parameters = Variables::from([(name.clone(), Item::from("bar").into())]);

        let mut state = State::new(xot, span_info, names);

        let mut xot = Xot::new();
        let variables =
            static_evaluate(&mut state, document_element, static_parameters, &mut xot).unwrap();
        assert_eq!(variables.len(), 1);

        assert_eq!(variables.get(&name), Some(&Item::from("bar").into()));
    }

    #[test]
    fn test_one_parameter_absent_not_required_with_select() {
        let xml = r#"
        <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
            <xsl:param name="x" static="yes" select="'foo'"/>
        </xsl:stylesheet>
        "#;
        let mut xot = Xot::new();
        let (root, span_info) = xot.parse_with_span_info(xml).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(root).unwrap();

        let name = xpath_ast::Name::name("x");
        let static_parameters = Variables::new();

        let mut state = State::new(xot, span_info, names);

        let mut xot = Xot::new();
        let variables =
            static_evaluate(&mut state, document_element, static_parameters, &mut xot).unwrap();
        assert_eq!(variables.len(), 1);

        assert_eq!(variables.get(&name), Some(&Item::from("foo").into()));
    }

    #[test]
    fn test_one_parameter_absent_no_select_without_as() {
        let xml = r#"
        <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
            <xsl:param name="x" static="yes" />
        </xsl:stylesheet>
        "#;
        let mut xot = Xot::new();
        let (root, span_info) = xot.parse_with_span_info(xml).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(root).unwrap();

        let name = xpath_ast::Name::name("x");
        let static_parameters = Variables::new();

        let mut state = State::new(xot, span_info, names);

        let mut xot = Xot::new();
        let variables =
            static_evaluate(&mut state, document_element, static_parameters, &mut xot).unwrap();
        assert_eq!(variables.len(), 1);

        assert_eq!(variables.get(&name), Some(&Item::from("").into()));
    }

    #[test]
    fn test_one_parameter_absent_no_select_with_as() {
        let xml = r#"
        <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
            <xsl:param name="x" static="yes" as="xs:integer" />
        </xsl:stylesheet>
        "#;
        let mut xot = Xot::new();
        let (root, span_info) = xot.parse_with_span_info(xml).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(root).unwrap();

        let name = xpath_ast::Name::name("x");
        let static_parameters = Variables::new();

        let mut state = State::new(xot, span_info, names);

        let mut xot = Xot::new();
        let variables =
            static_evaluate(&mut state, document_element, static_parameters, &mut xot).unwrap();
        assert_eq!(variables.len(), 1);

        assert_eq!(variables.get(&name), Some(&Sequence::default()));
    }

    #[test]
    fn test_use_when_false_on_top_level() {
        let xml = r#"
        <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
            <xsl:if use-when="false()"/>
        </xsl:stylesheet>
        "#;
        let mut xot = Xot::new();
        let (root, span_info) = xot.parse_with_span_info(xml).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(root).unwrap();

        let mut state = State::new(xot, span_info, names);

        let mut xot = Xot::new();
        static_evaluate(&mut state, document_element, Variables::new(), &mut xot).unwrap();
        assert_eq!(
            state.xot.to_string(document_element).unwrap(),
            "<xsl:stylesheet xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\" version=\"3.0\"/>"
        );
    }

    #[test]
    fn test_use_when_true_on_top_level() {
        let xml = r#"
        <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
            <xsl:if use-when="true()"/>
        </xsl:stylesheet>
        "#;
        let mut xot = Xot::new();
        let (root, span_info) = xot.parse_with_span_info(xml).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(root).unwrap();

        let mut state = State::new(xot, span_info, names);

        let mut xot = Xot::new();
        static_evaluate(&mut state, document_element, Variables::new(), &mut xot).unwrap();
        assert_eq!(
            state.xot.to_string(document_element).unwrap(),
            r#"<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0"><xsl:if use-when="true()"/></xsl:stylesheet>"#
        );
    }

    #[test]
    fn test_use_when_depends_on_variable() {
        let xml = r#"
        <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
            <xsl:variable name="x" static="yes" select="false()"/>
            <foo xsl:use-when="$x"/>
        </xsl:stylesheet>
        "#;
        let mut xot = Xot::new();
        let (root, span_info) = xot.parse_with_span_info(xml).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(root).unwrap();

        let mut state = State::new(xot, span_info, names);

        let mut xot = Xot::new();
        static_evaluate(&mut state, document_element, Variables::new(), &mut xot).unwrap();
        assert_eq!(
            state.xot.to_string(document_element).unwrap(),
            r#"<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0"><xsl:variable name="x" static="yes" select="false()"/></xsl:stylesheet>"#
        );
    }

    #[test]
    fn test_use_when_depends_on_initial_static_variable() {
        let xml = r#"
        <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
            <foo xsl:use-when="$x"/>
        </xsl:stylesheet>
        "#;
        let mut xot = Xot::new();
        let (root, span_info) = xot.parse_with_span_info(xml).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(root).unwrap();

        let mut state = State::new(xot, span_info, names);
        let initial_static_variables = Variables::from([(xpath_ast::Name::name("x"), Item::from(false).into())]);

        let mut xot = Xot::new();
        static_evaluate_with_initial_variables(
            &mut state,
            document_element,
            initial_static_variables,
            Variables::new(),
            None,
            None,
            None,
            &mut xot,
        )
        .unwrap();
        assert_eq!(
            state.xot.to_string(document_element).unwrap(),
            r#"<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0"/>"#
        );
    }

    #[test]
    fn test_xsl_use_when_false_on_top_level() {
        let xml = r#"
        <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
            <foo xsl:use-when="false()"/>
        </xsl:stylesheet>
        "#;
        let mut xot = Xot::new();
        let (root, span_info) = xot.parse_with_span_info(xml).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(root).unwrap();

        let mut state = State::new(xot, span_info, names);

        let mut xot = Xot::new();
        static_evaluate(&mut state, document_element, Variables::new(), &mut xot).unwrap();
        assert_eq!(
            state.xot.to_string(document_element).unwrap(),
            "<xsl:stylesheet xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\" version=\"3.0\"/>"
        );
    }

    #[test]
    fn test_use_when_false_for_xsl_param() {
        let xml = r#"
        <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
            <xsl:param name="x" static="yes" select="'foo'" use-when="false()"/>
        </xsl:stylesheet>
        "#;
        let mut xot = Xot::new();
        let (root, span_info) = xot.parse_with_span_info(xml).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(root).unwrap();

        let mut state = State::new(xot, span_info, names);
        let mut xot = Xot::new();
        let variables =
            static_evaluate(&mut state, document_element, Variables::new(), &mut xot).unwrap();
        assert_eq!(variables.len(), 0);
    }

    #[test]
    fn test_xpath_default_namespace() {
        let xslt = r#"
        <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
            xpath-default-namespace="http://www.w3.org/1999/xhtml"
            version="3.0">
            <xsl:param name="x" static="yes"/>
            <xsl:variable name="y" static="yes" select="$x/html/body/p/string()"/>
        </xsl:stylesheet>"#;

        let xhtml = r#"
        <html xmlns="http://www.w3.org/1999/xhtml">
          <body>
            <p>foo</p>
          </body>
        </html>"#;

        let mut xot = Xot::new();
        let (xslt, span_info) = xot.parse_with_span_info(xslt).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(xslt).unwrap();

        let mut state = State::new(xot, span_info, names);

        let mut xot = Xot::new();
        let xhtml = xot.parse(xhtml).unwrap();
        let parameters = Variables::from([(xpath_ast::Name::name("x"), Item::Node(xhtml).into())]);
        let variables =
            static_evaluate(&mut state, document_element, parameters, &mut xot).unwrap();
        assert_eq!(variables.len(), 2);
        let y = xpath_ast::Name::name("y");
        assert_eq!(variables.get(&y), Some(&Item::from("foo").into()));
    }

    #[test]
    fn test_xpath_default_namespace_on_declaration() {
        let xslt = r#"
        <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
            version="3.0">
            <xsl:param name="x" static="yes"/>
            <xsl:variable name="y" xpath-default-namespace="http://www.w3.org/1999/xhtml" static="yes" select="$x/html/body/p/string()"/>
        </xsl:stylesheet>"#;

        let xhtml = r#"
        <html xmlns="http://www.w3.org/1999/xhtml">
          <body>
            <p>foo</p>
          </body>
        </html>"#;

        let mut xot = Xot::new();
        let (xslt, span_info) = xot.parse_with_span_info(xslt).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(xslt).unwrap();

        let mut state = State::new(xot, span_info, names);

        let mut xot = Xot::new();
        let xhtml = xot.parse(xhtml).unwrap();
        let parameters = Variables::from([(xpath_ast::Name::name("x"), Item::Node(xhtml).into())]);
        let variables =
            static_evaluate(&mut state, document_element, parameters, &mut xot).unwrap();
        assert_eq!(variables.len(), 2);
        let y = xpath_ast::Name::name("y");
        assert_eq!(variables.get(&y), Some(&Item::from("foo").into()));
    }

    #[test]
    fn test_use_when_false_on_top_node_keeps_top_level_children() {
        let xml = r#"
        <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0" use-when="false()">
           <foo/>
        </xsl:stylesheet>
        "#;
        let mut xot = Xot::new();
        let (root, span_info) = xot.parse_with_span_info(xml).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(root).unwrap();

        let mut state = State::new(xot, span_info, names);

        let mut xot = Xot::new();
        static_evaluate(&mut state, document_element, Variables::new(), &mut xot).unwrap();
        assert_eq!(
            state.xot.to_string(document_element).unwrap(),
            r#"<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0" use-when="false()"><foo/></xsl:stylesheet>"#
        );
    }

     #[test]
    fn test_use_when_false_on_transform_root_keeps_top_level_children() {
          let xml = r#"
          <t:transform xmlns:t="http://www.w3.org/1999/XSL/Transform" version="2.0" use-when="false()">
              <t:template match="elem">
                  <out>
                      <t:copy>
                          <t:apply-templates/>
                      </t:copy>
                  </out>
              </t:template>

              <t:template match="a | b" use-when="true()">
                  <print>
                      <t:next-match/>
                  </print>
              </t:template>
          </t:transform>
          "#;
          let mut xot = Xot::new();
          let (root, span_info) = xot.parse_with_span_info(xml).unwrap();
          let names = Names::new(&mut xot);
          let document_element = xot.document_element(root).unwrap();

          let mut state = State::new(xot, span_info, names);

          let mut xot = Xot::new();
          static_evaluate(&mut state, document_element, Variables::new(), &mut xot).unwrap();
          assert_eq!(
                state.xot.to_string(document_element).unwrap(),
            r#"<t:transform xmlns:t="http://www.w3.org/1999/XSL/Transform" version="2.0" use-when="false()"><t:template match="elem"><out><t:copy><t:apply-templates/></t:copy></out></t:template><t:template match="a | b" use-when="true()"><print><t:next-match/></print></t:template></t:transform>"#
          );
     }

    #[test]
    fn test_use_when_on_other_content() {
        let xml = r#"
        <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
           <foo><xsl:if use-when="false()"/></foo>
        </xsl:stylesheet>
        "#;
        let mut xot = Xot::new();
        let (root, span_info) = xot.parse_with_span_info(xml).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(root).unwrap();

        let mut state = State::new(xot, span_info, names);

        let mut xot = Xot::new();
        static_evaluate(&mut state, document_element, Variables::new(), &mut xot).unwrap();
        assert_eq!(
            state.xot.to_string(document_element).unwrap(),
            r#"<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0"><foo/></xsl:stylesheet>"#
        );
    }

    #[test]
    fn test_xsl_use_when_on_other_content() {
        let xml = r#"
        <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0">
           <foo><bar xsl:use-when="false()"/></foo>
        </xsl:stylesheet>
        "#;
        let mut xot = Xot::new();
        let (root, span_info) = xot.parse_with_span_info(xml).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(root).unwrap();

        let mut state = State::new(xot, span_info, names);

        let mut xot = Xot::new();
        static_evaluate(&mut state, document_element, Variables::new(), &mut xot).unwrap();
        assert_eq!(
            state.xot.to_string(document_element).unwrap(),
            r#"<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0"><foo/></xsl:stylesheet>"#
        );
    }

    #[test]
    fn test_nested_use_when() {
        let xml = r#"<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:if use-when="false()"><xsl:if use-when="false()"><p/></xsl:if></xsl:if></xsl:transform>"#;
        let mut xot = Xot::new();
        let (root, span_info) = xot.parse_with_span_info(xml).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(root).unwrap();

        let mut state = State::new(xot, span_info, names);

        let mut xot = Xot::new();
        static_evaluate(&mut state, document_element, Variables::new(), &mut xot).unwrap();
        assert_eq!(
            state.xot.to_string(document_element).unwrap(),
            r#"<xsl:transform xmlns:xsl="http://www.w3.org/1999/XSL/Transform"/>"#
        );
    }

    #[test]
    fn test_use_when_on_other_content_default_element_namespace() {
        let xslt = r#"
        <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
            version="3.0">
            <xsl:param name="x" static="yes"/>
            <foo xsl:xpath-default-namespace="http://www.w3.org/1999/xhtml"><bar xsl:use-when="$x/html/body/p/string() = 'bar'"/></foo>
        </xsl:stylesheet>"#;

        let xhtml = r#"
        <html xmlns="http://www.w3.org/1999/xhtml">
          <body>
            <p>foo</p>
          </body>
        </html>"#;

        let mut xot = Xot::new();
        let (xslt, span_info) = xot.parse_with_span_info(xslt).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(xslt).unwrap();

        let mut state = State::new(xot, span_info, names);

        let mut xot = Xot::new();
        let xhtml = xot.parse(xhtml).unwrap();
        let parameters = Variables::from([(xpath_ast::Name::name("x"), Item::Node(xhtml).into())]);
        static_evaluate(&mut state, document_element, parameters, &mut xot).unwrap();
        assert_eq!(
            state.xot.to_string(document_element).unwrap(),
            r#"<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0"><xsl:param name="x" static="yes"/><foo xsl:xpath-default-namespace="http://www.w3.org/1999/xhtml"/></xsl:stylesheet>"#
        );
    }

    #[test]
    fn test_use_when_on_other_content_default_element_namespace_included() {
        let xslt = r#"
        <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
            version="3.0">
            <xsl:param name="x" static="yes"/>
            <foo xsl:xpath-default-namespace="http://www.w3.org/1999/xhtml"><bar xsl:use-when="$x/html/body/p/string() = 'foo'"/></foo>
        </xsl:stylesheet>"#;

        let xhtml = r#"
        <html xmlns="http://www.w3.org/1999/xhtml">
          <body>
            <p>foo</p>
          </body>
        </html>"#;

        let mut xot = Xot::new();
        let (xslt, span_info) = xot.parse_with_span_info(xslt).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(xslt).unwrap();

        let mut state = State::new(xot, span_info, names);

        let mut xot = Xot::new();
        let xhtml = xot.parse(xhtml).unwrap();
        let parameters = Variables::from([(xpath_ast::Name::name("x"), Item::Node(xhtml).into())]);
        static_evaluate(&mut state, document_element, parameters, &mut xot).unwrap();
        assert_eq!(
            state.xot.to_string(document_element).unwrap(),
            r#"<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0"><xsl:param name="x" static="yes"/><foo xsl:xpath-default-namespace="http://www.w3.org/1999/xhtml"><bar xsl:use-when="$x/html/body/p/string() = &apos;foo&apos;"/></foo></xsl:stylesheet>"#
        );
    }

    #[test]
    fn test_use_when_with_namespace_prefix_defined_lower_down() {
        let xslt = r#"
        <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
            version="3.0">
            <xsl:param name="x" static="yes"/>
            <foo xmlns:xhtml="http://www.w3.org/1999/xhtml"><bar xsl:use-when="$x/xhtml:html/xhtml:body/xhtml:p/string() = 'foo'"/></foo>
        </xsl:stylesheet>"#;

        let xhtml = r#"
        <html xmlns="http://www.w3.org/1999/xhtml">
          <body>
            <p>foo</p>
          </body>
        </html>"#;

        let mut xot = Xot::new();
        let (xslt, span_info) = xot.parse_with_span_info(xslt).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(xslt).unwrap();

        let mut state = State::new(xot, span_info, names);

        let mut xot = Xot::new();
        let xhtml = xot.parse(xhtml).unwrap();
        let parameters = Variables::from([(xpath_ast::Name::name("x"), Item::Node(xhtml).into())]);
        static_evaluate(&mut state, document_element, parameters, &mut xot).unwrap();
        assert_eq!(
            state.xot.to_string(document_element).unwrap(),
            r#"<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0"><xsl:param name="x" static="yes"/><foo xmlns:xhtml="http://www.w3.org/1999/xhtml"><bar xsl:use-when="$x/xhtml:html/xhtml:body/xhtml:p/string() = &apos;foo&apos;"/></foo></xsl:stylesheet>"#
        );
    }

    #[test]
    fn test_use_when_with_namespace_prefix_defined_element_itself() {
        let xslt = r#"
        <xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
            version="3.0">
            <xsl:param name="x" static="yes"/>
            <foo ><bar xmlns:xhtml="http://www.w3.org/1999/xhtml" xsl:use-when="$x/xhtml:html/xhtml:body/xhtml:p/string() = 'foo'"/></foo>
        </xsl:stylesheet>"#;

        let xhtml = r#"
        <html xmlns="http://www.w3.org/1999/xhtml">
          <body>
            <p>foo</p>
          </body>
        </html>"#;

        let mut xot = Xot::new();
        let (xslt, span_info) = xot.parse_with_span_info(xslt).unwrap();
        let names = Names::new(&mut xot);
        let document_element = xot.document_element(xslt).unwrap();

        let mut state = State::new(xot, span_info, names);

        let mut xot = Xot::new();
        let xhtml = xot.parse(xhtml).unwrap();
        let parameters = Variables::from([(xpath_ast::Name::name("x"), Item::Node(xhtml).into())]);
        static_evaluate(&mut state, document_element, parameters, &mut xot).unwrap();
        assert_eq!(
            state.xot.to_string(document_element).unwrap(),
            r#"<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="3.0"><xsl:param name="x" static="yes"/><foo><bar xmlns:xhtml="http://www.w3.org/1999/xhtml" xsl:use-when="$x/xhtml:html/xhtml:body/xhtml:p/string() = &apos;foo&apos;"/></foo></xsl:stylesheet>"#
        );
    }

    // TODO:
    // - weirdness of the parameter xot versus the parser xot; I'm not
    // sure it's sustainable
    // - shadow attributes support
    // - shadow attributes for use-when in particular
}
