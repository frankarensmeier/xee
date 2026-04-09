use std::fs;

use iri_string::types::{IriReferenceStr, IriString};
use xee_xpath_ast::{ast, pattern, Pattern};
use xot::Xot;

use crate::function;
use crate::interpreter::Interpreter;
use crate::pattern::pattern_core::PredicateMatcher;
use crate::sequence::{Item, Sequence};

#[derive(Debug, Default)]
pub struct PatternLookup<V: Clone> {
    pub(crate) patterns: Vec<(Pattern<function::InlineFunctionId>, V)>,
}

pub(crate) struct InterpreterPredicateMatcher<'a> {
    interpreter: &'a mut Interpreter<'a>,
}

impl<'a> InterpreterPredicateMatcher<'a> {
    pub(crate) fn new(interpreter: &'a mut Interpreter<'a>) -> Self {
        Self { interpreter }
    }
}

impl PredicateMatcher for Interpreter<'_> {
    fn match_predicate_with_context(
        &mut self,
        inline_function_id: function::InlineFunctionId,
        item: &Item,
        position: usize,
        size: usize,
    ) -> bool {
        let function = function::InlineFunctionData::new(inline_function_id, Vec::new()).into();
        let arguments = [
            item.clone().into(),
            (position as u64).into(),
            (size as u64).into(),
        ];

        // the specification says to swallow any errors
        // TODO: log errors somehow here?
        let value = self.call_function_with_arguments(&function, &arguments);
        if let Ok(value) = value {
            value.effective_boolean_value().unwrap_or(false)
        } else {
            false
        }
    }

    fn xot(&self) -> &Xot {
        self.xot()
    }

    fn rooted_pattern_sequence(&mut self, root: &pattern::RootExpr) -> Option<Sequence> {
        match root {
            pattern::RootExpr::VarRef(name) => self.lookup_root_variable(name),
            pattern::RootExpr::FunctionCall(function_call) => {
                self.lookup_root_function(function_call)
            }
        }
    }
}

impl Interpreter<'_> {
    fn lookup_root_variable(&mut self, name: &ast::Name) -> Option<Sequence> {
        if let Some(sequence) = self.runnable().dynamic_context().variables().get(name) {
            return Some(sequence.clone());
        }

        let declarations = &self.runnable().program().declarations;
        declarations
            .global_variables
            .iter()
            .position(|global| global.original_name.as_ref() == Some(name))
            .and_then(|index| self.resolve_global_variable(index).ok())
    }

    fn lookup_root_function(&mut self, function_call: &pattern::FunctionCall) -> Option<Sequence> {
        match function_call.name {
            pattern::OuterFunctionName::Doc => self.lookup_root_doc(function_call),
            pattern::OuterFunctionName::Id
            | pattern::OuterFunctionName::ElementWithId
            | pattern::OuterFunctionName::Key
            | pattern::OuterFunctionName::Root => None,
        }
    }

    fn lookup_root_doc(&mut self, function_call: &pattern::FunctionCall) -> Option<Sequence> {
        let [uri_argument] = function_call.args.as_slice() else {
            return None;
        };
        let uri = self.resolve_pattern_argument_string(uri_argument)?;
        let document_node = self.load_pattern_document(&uri)?;
        Some(Sequence::from(vec![Item::from(document_node)]))
    }

    fn resolve_pattern_argument_string(&mut self, argument: &pattern::Argument) -> Option<String> {
        match argument {
            pattern::Argument::Literal(literal) => Some(literal_to_string(literal)),
            pattern::Argument::VarRef(name) => self
                .lookup_root_variable(name)
                .and_then(|sequence| sequence.string_value(self.xot()).ok()),
        }
    }

    fn load_pattern_document(&mut self, uri: &str) -> Option<xot::Node> {
        let iri_reference: &IriReferenceStr = uri.try_into().ok()?;
        let uri = self.absolute_pattern_uri(iri_reference)?;

        let documents = self.runnable().dynamic_context().documents();
        if let Some(document) = documents.borrow().get_by_uri(&uri) {
            return Some(document.root());
        }

        let url = url::Url::parse(uri.as_str()).ok()?;
        let path = url.to_file_path().ok()?;
        let xml = fs::read_to_string(&path).ok()?;

        let handle = documents
            .borrow_mut()
            .add_string(self.xot_mut(), Some(uri.as_ref()), &xml)
            .ok()?;
        let node = documents.borrow().get_node_by_handle(handle);
        node
    }

    fn absolute_pattern_uri(&self, uri: &IriReferenceStr) -> Option<IriString> {
        match uri.to_iri() {
            Ok(iri) => Some(iri.into()),
            Err(relative_iri) => {
                let base = self
                    .runnable()
                    .dynamic_context()
                    .static_context()
                    .static_base_uri()?;
                Some(relative_iri.resolve_against(base).into())
            }
        }
    }
}

fn literal_to_string(literal: &ast::Literal) -> String {
    match literal {
        ast::Literal::Decimal(value) => value.to_string(),
        ast::Literal::Integer(value) => value.to_string(),
        ast::Literal::Double(value) => value.to_string(),
        ast::Literal::String(value) => value.clone(),
    }
}

impl<V: Clone> PatternLookup<V> {
    pub(crate) fn new() -> Self {
        Self {
            patterns: Vec::new(),
        }
    }

    pub(crate) fn add_rules(&mut self, rules: Vec<(Pattern<function::InlineFunctionId>, V)>) {
        self.patterns.extend(rules);
    }

    pub(crate) fn lookup(
        &self,
        mut matches: impl FnMut(&Pattern<function::InlineFunctionId>) -> bool,
    ) -> Option<&V> {
        self.patterns
            .iter()
            .find(|(pattern, _)| matches(pattern))
            .map(|(_, value)| value)
    }

    pub(crate) fn lookup_with_ambiguity(
        &self,
        mut matches: impl FnMut(&Pattern<function::InlineFunctionId>) -> bool,
        same_rank: impl Fn(&V, &V) -> bool,
    ) -> Option<(&V, bool)> {
        let mut first = None;
        for (pattern, value) in &self.patterns {
            if !matches(pattern) {
                continue;
            }
            if let Some(first_value) = first {
                return Some((first_value, same_rank(first_value, value)));
            }
            first = Some(value);
        }
        first.map(|value| (value, false))
    }

    pub(crate) fn lookup_after(
        &self,
        mut matches: impl FnMut(&Pattern<function::InlineFunctionId>) -> bool,
        is_current: impl Fn(&V) -> bool,
    ) -> Option<&V> {
        let mut seen_current = false;
        for (pattern, value) in &self.patterns {
            if !matches(pattern) {
                continue;
            }
            if !seen_current {
                if is_current(value) {
                    seen_current = true;
                }
                continue;
            }
            return Some(value);
        }
        None
    }

    pub(crate) fn lookup_after_lower_import_precedence(
        &self,
        current_import_precedence: i64,
        mut matches: impl FnMut(&Pattern<function::InlineFunctionId>) -> bool,
        is_current: impl Fn(&V) -> bool,
        import_precedence_of: impl Fn(&V) -> i64,
        is_eligible: impl Fn(&V) -> bool,
    ) -> Option<&V> {
        let mut seen_current = false;
        for (pattern, value) in &self.patterns {
            if !matches(pattern) {
                continue;
            }
            if !seen_current {
                if is_current(value) {
                    seen_current = true;
                }
                continue;
            }
            if import_precedence_of(value) < current_import_precedence && is_eligible(value) {
                return Some(value);
            }
        }
        None
    }
}
