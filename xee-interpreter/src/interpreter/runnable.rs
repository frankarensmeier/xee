use std::rc::Rc;

use ibig::ibig;
use iri_string::types::IriReferenceStr;
use xot::Xot;

use crate::context::DocumentsRef;
use crate::context::DynamicContext;
use crate::context::StaticContext;
use crate::error::SpannedError;
use crate::function::{Function, InlineFunctionData};
use crate::interpreter::interpret::ContextInfo;
use crate::sequence;
use crate::stack;
use crate::{error, string};

use super::program::FunctionInfo;
use super::program::InitialFocusMode;
use super::Interpreter;
use super::Program;

#[derive(Debug)]
pub struct Runnable<'a> {
    program: &'a Program,
    // TODO: this should be private, but is needed right now
    // to implement call_static without lifetime issues.
    // We could possibly obtain context from the interpreter directly,
    // but this leads to lifetime issues right now.
    pub(crate) dynamic_context: &'a DynamicContext<'a>,
}

impl<'a> Runnable<'a> {
    pub(crate) fn new(program: &'a Program, dynamic_context: &'a DynamicContext) -> Self {
        Self {
            program,
            dynamic_context,
        }
    }

    fn run_value(&self, xot: &'a mut Xot) -> error::SpannedResult<stack::Value> {
        if self.dynamic_context.context_item().is_none()
            && self
                .program
                .declarations
                .named_template_by_name("initial-template")
                .is_some()
        {
            let sequence = self.run_named_template_value("initial-template", xot)?;
            return Ok(construct_complex_content(xot, sequence).into());
        }

        let arguments = self.dynamic_context.arguments().unwrap();
        let mut interpreter = Interpreter::new(self, xot);

        let context_info = if let Some(context_item) = self.dynamic_context.context_item() {
            match self.program.initial_focus_mode() {
                InitialFocusMode::Full => ContextInfo {
                    item: context_item.clone().into(),
                    position: ibig!(1).into(),
                    size: ibig!(1).into(),
                },
                InitialFocusMode::ItemOnly => ContextInfo {
                    item: context_item.clone().into(),
                    position: stack::Value::Absent,
                    size: stack::Value::Absent,
                },
            }
        } else {
            ContextInfo {
                item: stack::Value::Absent,
                position: stack::Value::Absent,
                size: stack::Value::Absent,
            }
        };

        interpreter.start(context_info, arguments);
        interpreter.run(0)?;

        let state = interpreter.state();
        // the stack has to be 1 values and return the result of the expression
        // why 1 value if the context item is on the top of the stack? This is because
        // the outer main function will pop the context item; this code is there to
        // remove the function id from the stack but the main function has no function id
        assert_eq!(
            state.stack().len(),
            1,
            "stack must only have 1 value but found {:?}",
            state.stack()
        );
        let value = state.stack().last().unwrap().clone();
        match value {
            stack::Value::Absent => Err(SpannedError {
                error: error::Error::XPDY0002,
                span: Some(self.program.span().into()),
                detail: None,

                contexts: Vec::new(),
            }),
            _ => Ok(value),
        }
    }

    fn run_named_template_value(
        &self,
        name: &str,
        xot: &'a mut Xot,
    ) -> error::SpannedResult<sequence::Sequence> {
        let named_template = self
            .program
            .declarations
            .named_template_by_name(name)
            .ok_or(SpannedError {
                error: error::Error::XTDE0040,
                span: Some(self.program.span().into()),
                detail: None,

                contexts: Vec::new(),
            })?;
        let function: Function =
            InlineFunctionData::new(named_template.function_id, Vec::new()).into();
        let mut interpreter = Interpreter::new(self, xot);
        let empty_params = crate::function::Map::new(Vec::new()).unwrap();
        let context_arguments = if let Some(context_item) = self.dynamic_context.context_item() {
            [
                Some(sequence::Sequence::from(context_item.clone())),
                Some(sequence::Sequence::from(1_i64)),
                Some(sequence::Sequence::from(1_i64)),
            ]
        } else {
            [None, None, None]
        };
        interpreter
            .call_template_with_params(&function, context_arguments, &empty_params, &empty_params)
            .map_err(|error| SpannedError {
                error,
                span: Some(self.program.span().into()),
                detail: None,

                contexts: Vec::new(),
            })
    }

    fn merge_principal_result_documents(
        &self,
        result: sequence::Sequence,
    ) -> error::SpannedResult<sequence::Sequence> {
        let principal_result_documents = self.dynamic_context.principal_result_documents();
        if principal_result_documents.len() > 1
            || (!principal_result_documents.is_empty() && !result.is_empty())
        {
            return Err(SpannedError {
                error: error::Error::XTDE1490,
                span: Some(self.program.span().into()),
                detail: None,

                contexts: Vec::new(),
            });
        }

        principal_result_documents
            .into_iter()
            .try_fold(result, |acc, sequence| acc.concat(sequence))
            .map_err(|error| SpannedError {
                error,
                span: Some(self.program.span().into()),
                detail: None,

                contexts: Vec::new(),
            })
    }

    /// Run the program against a sequence item.
    pub fn many(&self, xot: &'a mut Xot) -> error::SpannedResult<sequence::Sequence> {
        // Apply xsl:strip-space to the initial source document before execution
        if self.program.declarations.strip_space_all {
            if let Some(context_item) = self.dynamic_context.context_item() {
                if let sequence::Item::Node(node) = context_item {
                    crate::library::strip_whitespace_only_text_children(xot, *node);
                }
            }
        }

        let result = self.run_value(xot)?.try_into()?;
        self.merge_principal_result_documents(result)
    }

    pub fn named_template(
        &self,
        name: &str,
        xot: &'a mut Xot,
    ) -> error::SpannedResult<sequence::Sequence> {
        let sequence = self.run_named_template_value(name, xot)?;
        let sequence = self.merge_principal_result_documents(sequence)?;
        Ok(construct_complex_content(xot, sequence))
    }

    /// Run the program, expect a single item as the result.
    pub fn one(&self, xot: &'a mut Xot) -> error::SpannedResult<sequence::Item> {
        let sequence = self.many(xot)?;
        sequence.one().map_err(|error| SpannedError {
            error,
            span: Some(self.program.span().into()),
                detail: None,

                contexts: Vec::new(),
        })
    }

    /// Run the program, expect an optional single item as the result.
    pub fn option(&self, xot: &'a mut Xot) -> error::SpannedResult<Option<sequence::Item>> {
        let sequence = self.many(xot)?;
        let items = sequence.iter();
        sequence::option(items).map_err(|error| SpannedError {
            error,
            span: Some(self.program.span().into()),
                detail: None,

                contexts: Vec::new(),
        })
    }

    pub(crate) fn program(&self) -> &'a Program {
        self.program
    }

    pub fn dynamic_context(&self) -> &'a DynamicContext<'_> {
        self.dynamic_context
    }

    pub fn documents(&self) -> DocumentsRef {
        self.dynamic_context.documents()
    }

    pub fn static_context(&self) -> &StaticContext {
        self.program.static_context()
    }

    pub fn default_collation_uri(&self) -> &IriReferenceStr {
        self.dynamic_context
            .static_context()
            .default_collation_uri()
    }

    pub fn default_collation(&self) -> error::Result<Rc<string::Collation>> {
        self.dynamic_context.static_context().default_collation()
    }

    pub fn implicit_timezone(&self) -> chrono::FixedOffset {
        self.dynamic_context.implicit_timezone()
    }

    pub fn function_info<'b>(&'b self, function: &'b Function) -> FunctionInfo<'b> {
        self.program.function_info(function)
    }
}

/// Merge adjacent text nodes and discard zero-length text nodes in a sequence.
/// Construct complex content per XSLT 3.0 §5.7.1.
///
/// Steps:
/// 1. Document nodes replaced by their children
/// 2. Atomic values cast to strings
/// 3. Adjacent strings concatenated with single space separator
/// 4a. Remaining strings converted to text nodes
/// 4b. Zero-length text nodes discarded
/// 4c. Adjacent text nodes merged
fn construct_complex_content(
    xot: &mut Xot,
    sequence: sequence::Sequence,
) -> sequence::Sequence {
    let mut result: Vec<sequence::Item> = Vec::new();
    let mut pending_text: Option<String> = None;
    let mut pending_strings: Vec<String> = Vec::new();

    for item in sequence.iter() {
        match item {
            sequence::Item::Node(node) => {
                match xot.value(node) {
                    xot::Value::Document => {
                        // Step 1: replace document node with its children
                        let children: Vec<_> = xot.children(node).collect();
                        for child in children {
                            process_complex_content_node(
                                xot,
                                child,
                                &mut result,
                                &mut pending_text,
                                &mut pending_strings,
                            );
                        }
                    }
                    _ => {
                        process_complex_content_node(
                            xot,
                            node,
                            &mut result,
                            &mut pending_text,
                            &mut pending_strings,
                        );
                    }
                }
            }
            sequence::Item::Atomic(atomic) => {
                // Step 2: cast atomic to string; step 3: collect adjacent strings
                pending_strings.push(atomic.string_value());
            }
            sequence::Item::Function(_) => {
                flush_pending_strings(&mut pending_strings, &mut pending_text);
                flush_pending_text(xot, &mut pending_text, &mut result);
                result.push(item.clone());
            }
        }
    }

    flush_pending_strings(&mut pending_strings, &mut pending_text);
    flush_pending_text(xot, &mut pending_text, &mut result);
    result.into()
}

fn process_complex_content_node(
    xot: &mut Xot,
    node: xot::Node,
    result: &mut Vec<sequence::Item>,
    pending_text: &mut Option<String>,
    pending_strings: &mut Vec<String>,
) {
    if let xot::Value::Text(text) = xot.value(node) {
        // Text node: flush pending strings first, then accumulate text
        flush_pending_strings(pending_strings, pending_text);
        let s = text.get();
        if !s.is_empty() {
            if let Some(ref mut p) = pending_text {
                p.push_str(s);
            } else {
                *pending_text = Some(s.to_string());
            }
        }
    } else {
        // Non-text node: flush everything, then add the node
        flush_pending_strings(pending_strings, pending_text);
        flush_pending_text(xot, pending_text, result);
        result.push(sequence::Item::Node(node));
    }
}

/// Step 3: concatenate adjacent strings with space, then merge into pending text.
fn flush_pending_strings(pending_strings: &mut Vec<String>, pending_text: &mut Option<String>) {
    if pending_strings.is_empty() {
        return;
    }
    let concatenated = pending_strings.join(" ");
    pending_strings.clear();
    if !concatenated.is_empty() {
        if let Some(ref mut p) = pending_text {
            p.push_str(&concatenated);
        } else {
            *pending_text = Some(concatenated);
        }
    }
}

/// Convert pending text to a text node (discarding if empty).
fn flush_pending_text(
    xot: &mut Xot,
    pending_text: &mut Option<String>,
    result: &mut Vec<sequence::Item>,
) {
    if let Some(s) = pending_text.take() {
        if !s.is_empty() {
            let text_node = xot.new_text(&s);
            result.push(sequence::Item::Node(text_node));
        }
    }
}
