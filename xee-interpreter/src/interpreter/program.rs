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
    pub context_item_supplied: bool,
    pub xpath_default_namespace: String,
    pub default_collation: String,
    pub namespace_context: Option<sequence::Item>,
    pub with_params: Option<function::Map>,
    pub base_uri: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitialFocusMode {
    Full,
    ItemOnly,
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

#[derive(Debug, Clone)]
pub struct SourceChunk {
    uri: String,
    start_offset: usize,
    end_offset: usize,
    source: Option<String>,
}

impl SourceChunk {
    fn len(&self) -> usize {
        self.end_offset.saturating_sub(self.start_offset)
    }
}

#[derive(Debug)]
pub struct Program {
    span: Span,
    source: Option<String>,
    source_chunks: Vec<SourceChunk>,
    pub functions: Vec<function::InlineFunction>,
    pub declarations: Declarations,
    static_context: context::StaticContext,
    dynamic_xpath_evaluator: Option<Box<dyn DynamicXPathEvaluator>>,
    transform_evaluator: Option<Box<dyn TransformEvaluator>>,
    initial_focus_mode: InitialFocusMode,
    map_signature: function::Signature,
    array_signature: function::Signature,
}

impl Program {
    pub fn new(static_context: context::StaticContext, span: Span) -> Self {
        Program {
            span,
            source: None,
            source_chunks: Vec::new(),
            functions: Vec::new(),
            declarations: Declarations::new(),
            static_context,
            dynamic_xpath_evaluator: None,
            transform_evaluator: None,
            initial_focus_mode: InitialFocusMode::Full,
            map_signature: function::Signature::map_signature(),
            array_signature: function::Signature::array_signature(),
        }
    }

    pub fn static_context(&self) -> &context::StaticContext {
        &self.static_context
    }

    pub fn set_dynamic_xpath_evaluator(&mut self, evaluator: Box<dyn DynamicXPathEvaluator>) {
        self.dynamic_xpath_evaluator = Some(evaluator);
    }

    pub fn dynamic_xpath_evaluator(&self) -> Option<&dyn DynamicXPathEvaluator> {
        self.dynamic_xpath_evaluator.as_deref()
    }

    pub fn set_transform_evaluator(&mut self, evaluator: Box<dyn TransformEvaluator>) {
        self.transform_evaluator = Some(evaluator);
    }

    pub fn transform_evaluator(&self) -> Option<&dyn TransformEvaluator> {
        self.transform_evaluator.as_deref()
    }

    pub fn set_initial_focus_mode(&mut self, initial_focus_mode: InitialFocusMode) {
        self.initial_focus_mode = initial_focus_mode;
    }

    pub fn initial_focus_mode(&self) -> InitialFocusMode {
        self.initial_focus_mode
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

    pub fn add_source_chunk(
        &mut self,
        uri: String,
        start_offset: usize,
        end_offset: usize,
        source: Option<String>,
    ) {
        self.source_chunks.push(SourceChunk {
            uri,
            start_offset,
            end_offset: end_offset.max(start_offset),
            source,
        });
    }

    pub fn source_for_uri(&self, uri: &str) -> Option<&str> {
        self.source_chunks
            .iter()
            .find(|chunk| chunk.uri == uri)
            .and_then(|chunk| chunk.source.as_deref())
    }

    pub fn resolve_source_span(
        &self,
        span: SourceSpan,
    ) -> Option<(String, std::ops::Range<usize>)> {
        let range = span.range();
        let start = range.start;
        let end = range.end;
        let chunk = self
            .source_chunks
            .iter()
            .find(|chunk| start >= chunk.start_offset && start <= chunk.end_offset)?;
        let local_start = start.saturating_sub(chunk.start_offset).min(chunk.len());
        let local_end = end.saturating_sub(chunk.start_offset).min(chunk.len());
        Some((chunk.uri.clone(), local_start..local_end))
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

    pub(crate) fn map_signature(&self) -> &function::Signature {
        &self.map_signature
    }

    pub(crate) fn array_signature(&self) -> &function::Signature {
        &self.array_signature
    }

    pub fn function_info<'a>(&'a self, function: &'a function::Function) -> FunctionInfo<'a> {
        let program = match function {
            function::Function::Inline(data) => data.program.as_deref().unwrap_or(self),
            _ => self,
        };
        FunctionInfo::new(function, program)
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
pub struct FunctionInfo<'a> {
    function: &'a function::Function,
    program: &'a Program,
}

impl<'a> FunctionInfo<'a> {
    pub(crate) fn new(function: &'a function::Function, program: &'a Program) -> FunctionInfo<'a> {
        FunctionInfo { function, program }
    }

    /// Return the arity of the function.
    pub fn arity(&self) -> usize {
        self.function.arity(self.program)
    }

    /// Return the name of the function.
    ///
    /// Note that only static functions have names.
    pub fn name(&self) -> Option<Name> {
        self.function.name(self.program)
    }

    /// Return the signature of the function.
    pub fn signature(&self) -> &'a function::Signature {
        self.function.signature(self.program)
    }
}
