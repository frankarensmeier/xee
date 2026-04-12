use crate::context;
use crate::declaration::Declarations;
use crate::error;
use crate::function;
use crate::sequence;
use crate::span::SourceSpan;
use xee_name::Name;
use xee_xpath_ast::ast::Span;

use super::Runnable;

#[derive(Debug, Clone)]
pub struct DynamicXPathRequest {
    pub xpath: String,
    pub context_item: Option<sequence::Item>,
    pub namespace_context: Option<sequence::Item>,
    pub with_params: Option<function::Map>,
}

pub trait DynamicXPathEvaluator: std::fmt::Debug {
    fn evaluate(
        &self,
        request: &DynamicXPathRequest,
        context: &context::DynamicContext,
        interpreter: &mut super::Interpreter<'_>,
    ) -> error::SpannedResult<sequence::Sequence>;
}

/// A request to evaluate fn:transform — run an XSLT transformation.
#[derive(Debug, Clone)]
pub struct TransformRequest {
    pub options: function::Map,
}

/// Trait for evaluating fn:transform. Implemented in xee-xslt-compiler
/// and injected into Program to break the dependency cycle.
pub trait TransformEvaluator: std::fmt::Debug {
    fn transform(
        &self,
        request: &TransformRequest,
        context: &context::DynamicContext,
        interpreter: &mut super::Interpreter<'_>,
    ) -> error::SpannedResult<function::Map>;
}

#[derive(Debug)]
pub struct Program {
    span: Span,
    source: Option<String>,
    pub functions: Vec<function::InlineFunction>,
    pub declarations: Declarations,
    static_context: context::StaticContext,
    dynamic_xpath_evaluator: Option<Box<dyn DynamicXPathEvaluator>>,
    transform_evaluator: Option<Box<dyn TransformEvaluator>>,
    map_signature: function::Signature,
    array_signature: function::Signature,
}

impl Program {
    pub fn new(static_context: context::StaticContext, span: Span) -> Self {
        Program {
            span,
            source: None,
            functions: Vec::new(),
            declarations: Declarations::new(),
            static_context,
            dynamic_xpath_evaluator: None,
            transform_evaluator: None,
            map_signature: function::Signature::map_signature(),
            array_signature: function::Signature::array_signature(),
        }
    }

    pub fn static_context(&self) -> &context::StaticContext {
        &self.static_context
    }

    pub fn set_dynamic_xpath_evaluator(
        &mut self,
        evaluator: Box<dyn DynamicXPathEvaluator>,
    ) {
        self.dynamic_xpath_evaluator = Some(evaluator);
    }

    pub fn dynamic_xpath_evaluator(&self) -> Option<&dyn DynamicXPathEvaluator> {
        self.dynamic_xpath_evaluator.as_deref()
    }

    pub fn set_transform_evaluator(
        &mut self,
        evaluator: Box<dyn TransformEvaluator>,
    ) {
        self.transform_evaluator = Some(evaluator);
    }

    pub fn transform_evaluator(&self) -> Option<&dyn TransformEvaluator> {
        self.transform_evaluator.as_deref()
    }

    pub fn dynamic_context_builder(&self) -> context::DynamicContextBuilder<'_> {
        context::DynamicContextBuilder::new(self)
    }

    pub fn span(&self) -> Span {
        self.span
    }

    pub fn set_source(&mut self, source: String) {
        self.source = Some(source);
    }

    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    pub fn source_location(&self, span: SourceSpan) -> Option<(usize, usize)> {
        let source = self.source()?;
        let offset = span.range().start.min(source.len());
        let mut line = 1usize;
        let mut line_start = 0usize;

        for (index, ch) in source.char_indices() {
            if index >= offset {
                break;
            }
            if ch == '\n' {
                line += 1;
                line_start = index + ch.len_utf8();
            }
        }

        let column = source[line_start..offset].chars().count() + 1;
        Some((line, column))
    }

    pub(crate) fn inline_function(
        &self,
        function_id: function::InlineFunctionId,
    ) -> &function::InlineFunction {
        &self.functions[function_id.0]
    }

    pub(crate) fn static_function(
        &self,
        function_id: function::StaticFunctionId,
    ) -> &function::StaticFunction {
        self.static_context.function_by_id(function_id)
    }

    pub fn function_info<'a, 'b>(
        &'a self,
        function: &'b function::Function,
    ) -> FunctionInfo<'a, 'b> {
        FunctionInfo::new(function, self)
    }

    /// Obtain a runnable version of this program, with a particular dynamic context.
    pub fn runnable<'a>(&'a self, dynamic_context: &'a context::DynamicContext) -> Runnable<'a> {
        Runnable::new(self, dynamic_context)
    }

    pub fn add_function(
        &mut self,
        function: function::InlineFunction,
    ) -> function::InlineFunctionId {
        let id = self.functions.len();
        if id > u16::MAX as usize {
            panic!("too many functions");
        }
        self.functions.push(function);

        function::InlineFunctionId(id)
    }

    pub(crate) fn get_function(&self, index: usize) -> &function::InlineFunction {
        &self.functions[index]
    }

    pub(crate) fn get_function_by_id(
        &self,
        id: function::InlineFunctionId,
    ) -> &function::InlineFunction {
        self.get_function(id.0)
    }

    pub(crate) fn main_id(&self) -> function::InlineFunctionId {
        function::InlineFunctionId(self.functions.len() - 1)
    }
}

/// Given a function provide information about it.
pub struct FunctionInfo<'a, 'b> {
    function: &'b function::Function,
    program: &'a Program,
}

impl<'a, 'b> FunctionInfo<'a, 'b> {
    pub(crate) fn new(
        function: &'b function::Function,
        program: &'a Program,
    ) -> FunctionInfo<'a, 'b> {
        FunctionInfo { function, program }
    }

    /// Return the arity of the function.
    pub fn arity(&self) -> usize {
        match self.function {
            function::Function::Inline(data) => self.program.inline_function(data.id).arity(),
            function::Function::Static(data) => self.program.static_function(data.id).arity(),
            function::Function::Array(_) => 1,
            function::Function::Map(_) => 1,
        }
    }

    /// Return the name of the function.
    ///
    /// Note that only static functions have names.
    pub fn name(&self) -> Option<Name> {
        match self.function {
            function::Function::Static(data) => {
                let static_function = self.program.static_function(data.id);
                static_function.name().cloned()
            }
            _ => None,
        }
    }

    /// Return the signature of the function.
    pub fn signature(&self) -> &'a function::Signature {
        match &self.function {
            function::Function::Static(data) => {
                let static_function = self.program.static_function(data.id);
                static_function.signature()
            }
            function::Function::Inline(data) => {
                let inline_function = self.program.inline_function(data.id);
                inline_function.signature()
            }
            function::Function::Map(_map) => &self.program.map_signature,
            function::Function::Array(_array) => &self.program.array_signature,
        }
    }
}
