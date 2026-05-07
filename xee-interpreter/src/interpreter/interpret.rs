use std::cmp::Ordering;
use std::rc::Rc;

use ibig::{ibig, IBig};

use xee_name::Name;
use xee_schema_type::Xs;
use xee_xpath_ast::ast;
use xot::xmlname::NameStrInfo;
use xot::Xot;

use crate::atomic::{self, AtomicCompare};
use crate::atomic::{
    op_add, op_div, op_idiv, op_mod, op_multiply, op_subtract, OpEq, OpGe, OpGt, OpLe, OpLt, OpNe,
};
use crate::context::DynamicContext;
use crate::declaration;
use crate::function;
use crate::library::accumulator_cache::AccumulatorCache;
use crate::library::key_cache::KeyCache;
use crate::library::number_count_cache::NumberCountCache;
use crate::pattern::pattern_lookup::NameCache;
use crate::pattern::PredicateMatcher;
use crate::sequence;
use crate::span::SourceSpan;
use crate::stack;
use crate::xml;
use crate::{error, pattern};

use super::instruction::{
    read_i16, read_instruction, read_u16, read_u8, EncodedInstruction, RaisedError,
};
use super::runnable::Runnable;
use super::state::State;

pub struct Interpreter<'a> {
    runnable: &'a Runnable<'a>,
    pub(crate) state: State<'a>,
    global_variables: Vec<GlobalValueState>,
    tunnel_params: Vec<function::Map>,
    mode_stack: Vec<pattern::ModeId>,
    template_rule_stack: Vec<function::InlineFunctionId>,
    template_rule_context_stack: Vec<TemplateRuleContext>,
    focus_absent_stack: Vec<bool>,
    error_contexts: Vec<error::ErrorContext>,
    /// Pre-resolved OwnedName → NameId cache for the current mode's patterns.
    /// Set before template lookups, cleared after. Avoids repeated
    /// OwnedName::maybe_to_ref (3 hash probes each) during pattern matching.
    pub(crate) name_cache: Option<Rc<NameCache>>,
    /// Prefix-sum cache for xsl:number level="any" — avoids O(n²) counting.
    pub(crate) number_count_cache: NumberCountCache,
    /// Index cache for xsl:key / fn:key() — avoids O(n) doc walk per call.
    pub(crate) key_cache: KeyCache,
    /// Accumulator value cache for xsl:accumulator — avoids repeated doc walks.
    pub(crate) accumulator_cache: AccumulatorCache,
}

#[derive(Clone)]
struct TemplateRuleContext {
    item: sequence::Item,
    position: usize,
    size: usize,
}

#[derive(Debug, Clone)]
enum GlobalValueState {
    Uninitialized,
    Resolving,
    Resolved(sequence::Sequence),
}

pub struct ContextInfo {
    pub item: stack::Value,
    pub position: stack::Value,
    pub size: stack::Value,
}

struct ApplyTemplatesOptions<'a> {
    params: &'a function::Map,
    tunnel_params: &'a function::Map,
    builtin_template_params_passthrough: bool,
}

impl From<sequence::Item> for ContextInfo {
    fn from(item: sequence::Item) -> Self {
        ContextInfo {
            item: item.into(),
            position: ibig!(1).into(),
            size: ibig!(1).into(),
        }
    }
}

impl<'a> Interpreter<'a> {
    pub fn new(runnable: &'a Runnable<'a>, xot: &'a mut Xot) -> Self {
        Interpreter {
            runnable,
            state: State::new(xot),
            global_variables: vec![
                GlobalValueState::Uninitialized;
                runnable.program().declarations.global_variables.len()
            ],
            tunnel_params: vec![function::Map::new(Vec::new()).unwrap()],
            mode_stack: Vec::new(),
            template_rule_stack: Vec::new(),
            template_rule_context_stack: Vec::new(),
            focus_absent_stack: Vec::new(),
            error_contexts: Vec::new(),
            name_cache: None,
            number_count_cache: NumberCountCache::new(),
            key_cache: KeyCache::new(),
            accumulator_cache: AccumulatorCache::new(),
        }
    }

    pub fn state(self) -> State<'a> {
        self.state
    }

    pub(crate) fn runnable(&self) -> &Runnable<'_> {
        self.runnable
    }

    pub fn start(&mut self, context_info: ContextInfo, arguments: Vec<sequence::Sequence>) {
        self.start_function(self.runnable.program().main_id(), context_info, arguments)
    }

    fn start_function(
        &mut self,
        function_id: function::InlineFunctionId,
        context_info: ContextInfo,
        arguments: Vec<sequence::Sequence>,
    ) {
        self.state.push_start_frame(function_id);

        self.push_context_info(context_info);
        // and any arguments
        for arg in arguments {
            self.state.push(arg);
        }
    }

    fn push_context_info(&mut self, context_info: ContextInfo) {
        self.state.push_value(context_info.item);
        self.state.push_value(context_info.position);
        self.state.push_value(context_info.size);
    }

    pub fn run(&mut self, start_base: usize) -> error::SpannedResult<()> {
        // annotate run with detailed error information
        self.run_actual(start_base).map_err(|e| self.err(e))
    }

    pub(crate) fn run_actual(&mut self, start_base: usize) -> error::Result<()> {
        // we can make this an infinite loop as all functions end
        // with the return instruction
        loop {
            let instruction = self.read_instruction();
            match instruction {
                EncodedInstruction::Add => {
                    self.arithmetic_with_offset(op_add)?;
                }
                EncodedInstruction::Sub => {
                    self.arithmetic_with_offset(op_subtract)?;
                }
                EncodedInstruction::Mul => {
                    self.arithmetic(op_multiply)?;
                }
                EncodedInstruction::Div => {
                    self.arithmetic(op_div)?;
                }
                EncodedInstruction::IntDiv => {
                    self.arithmetic(op_idiv)?;
                }
                EncodedInstruction::Mod => {
                    self.arithmetic(op_mod)?;
                }
                EncodedInstruction::Plus => {
                    self.unary_arithmetic(|a| a.plus())?;
                }
                EncodedInstruction::Minus => {
                    self.unary_arithmetic(|a| a.minus())?;
                }
                EncodedInstruction::Concat => {
                    let (a, b) = self.pop_atomic2_option()?;
                    let a = a.unwrap_or("".into());
                    let b = b.unwrap_or("".into());
                    let a = a.cast_to_string();
                    let b = b.cast_to_string();
                    let a = a.to_str().unwrap();
                    let b = b.to_str().unwrap();
                    let result = a.to_string() + b;
                    let item: sequence::Item = result.into();
                    self.state.push(item);
                }
                EncodedInstruction::Absent => {
                    self.state.push_value(stack::Value::Absent);
                }
                EncodedInstruction::Const => {
                    let index = self.read_u16();
                    self.state
                        .push(self.current_inline_function().constants[index as usize].clone());
                }
                EncodedInstruction::Closure => {
                    let function_id = self.read_u16();
                    let inline_function_id = function::InlineFunctionId(function_id as usize);
                    let closure_function = self.current_program().inline_function(inline_function_id);

                    let mut closure_vars = Vec::with_capacity(closure_function.closure_names.len());
                    for _ in 0..closure_function.closure_names.len() {
                        closure_vars.push(self.state.pop_value());
                    }
                    let function = self.inline_function_value(inline_function_id, closure_vars);
                    let item: sequence::Item = function.into();
                    self.state.push(item);
                }
                EncodedInstruction::NamedTemplate => {
                    let template_id = self.read_u16();
                    let named_template = self
                        .runnable
                        .program()
                        .declarations
                        .named_template(template_id as usize);
                    let function =
                        self.inline_function_value(named_template.function_id, Vec::new());
                    let item: sequence::Item = function.into();
                    self.state.push(item);
                }
                EncodedInstruction::StaticClosure => {
                    let static_function_id = self.read_u16();
                    let static_function_id =
                        function::StaticFunctionId(static_function_id as usize);
                    let static_closure =
                        self.create_static_closure_from_stack(static_function_id)?;
                    let item: sequence::Item = static_closure.into();
                    self.state.push(item);
                }
                EncodedInstruction::Var => {
                    let index = self.read_u16();
                    self.state.push_var(index as usize);
                }
                EncodedInstruction::GlobalVar => {
                    let index = self.read_u16();
                    let value = self.resolve_global_variable(index as usize)?;
                    self.state.push(value);
                }
                EncodedInstruction::VarIsAbsent => {
                    let index = self.read_u16();
                    self.state.push(self.state.var_is_absent(index as usize));
                }
                EncodedInstruction::Set => {
                    let index = self.read_u16();
                    self.state.set_var(index as usize);
                }
                EncodedInstruction::ClosureVar => {
                    let index = self.read_u16();
                    self.state.push_closure_var(index as usize)?;
                }
                EncodedInstruction::Comma => {
                    let b = self.state.pop()?;
                    let a = self.state.pop()?;
                    let sequence = a.concat(b)?;
                    self.state.push(sequence);
                }
                EncodedInstruction::CurlyArray => {
                    let sequence = self.state.pop()?;
                    let array: function::Array = sequence.into();
                    self.state.push(array);
                }
                EncodedInstruction::SquareArray => {
                    let length = self.pop_atomic().unwrap();
                    let length = length.cast_to_integer_value::<i64>()?;
                    let mut popped: Vec<sequence::Sequence> = Vec::with_capacity(length as usize);
                    for _ in 0..length {
                        popped.push(self.state.pop()?);
                    }
                    let item: sequence::Item = function::Array::new(popped).into();
                    self.state.push(item);
                }
                EncodedInstruction::CurlyMap => {
                    let length = self.pop_atomic().unwrap();
                    let length = length.cast_to_integer_value::<i64>()?;
                    let mut popped: Vec<(atomic::Atomic, sequence::Sequence)> =
                        Vec::with_capacity(length as usize);
                    for _ in 0..length {
                        let value = self.state.pop()?;
                        let key = self.pop_atomic()?;
                        popped.push((key, value));
                    }
                    let item: sequence::Item = function::Map::new(popped)?.into();
                    self.state.push(item);
                }
                EncodedInstruction::Jump => {
                    let displacement = self.read_i16();
                    self.state.jump(displacement as i32);
                }
                EncodedInstruction::JumpIfTrue => {
                    let displacement = self.read_i16();
                    let a = self.pop_effective_boolean()?;
                    if a {
                        self.state.jump(displacement as i32);
                    }
                }
                EncodedInstruction::JumpIfFalse => {
                    let displacement = self.read_i16();
                    let a = self.pop_effective_boolean()?;
                    if !a {
                        self.state.jump(displacement as i32);
                    }
                }
                EncodedInstruction::Eq => {
                    self.value_compare(OpEq)?;
                }
                EncodedInstruction::Ne => self.value_compare(OpNe)?,
                EncodedInstruction::Lt => {
                    self.value_compare(OpLt)?;
                }
                EncodedInstruction::Le => {
                    self.value_compare(OpLe)?;
                }
                EncodedInstruction::Gt => {
                    self.value_compare(OpGt)?;
                }
                EncodedInstruction::Ge => {
                    self.value_compare(OpGe)?;
                }
                EncodedInstruction::GenEq => {
                    self.general_compare(OpEq)?;
                }
                EncodedInstruction::GenNe => {
                    self.general_compare(OpNe)?;
                }
                EncodedInstruction::GenLt => {
                    self.general_compare(OpLt)?;
                }
                EncodedInstruction::GenLe => {
                    self.general_compare(OpLe)?;
                }
                EncodedInstruction::GenGt => {
                    self.general_compare(OpGt)?;
                }
                EncodedInstruction::GenGe => {
                    self.general_compare(OpGe)?;
                }
                EncodedInstruction::Is => {
                    let b = self.state.pop()?;
                    let a = self.state.pop()?;
                    if a.is_empty() || b.is_empty() {
                        self.state.push(sequence::Sequence::default());
                        continue;
                    }
                    let result = a.is(&b)?;
                    self.state.push(result);
                }
                EncodedInstruction::Precedes => {
                    let b = self.state.pop()?;
                    let a = self.state.pop()?;
                    if a.is_empty() || b.is_empty() {
                        self.state.push(sequence::Sequence::default());
                        continue;
                    }
                    let result = a.precedes(
                        &b,
                        self.runnable
                            .documents()
                            .borrow()
                            .document_order_access(self.xot()),
                    )?;
                    self.state.push(result);
                }
                EncodedInstruction::Follows => {
                    let b = self.state.pop()?;
                    let a = self.state.pop()?;
                    if a.is_empty() || b.is_empty() {
                        self.state.push(sequence::Sequence::default());
                        continue;
                    }
                    let result = a.follows(
                        &b,
                        self.runnable
                            .documents()
                            .borrow()
                            .document_order_access(self.xot()),
                    )?;
                    self.state.push(result);
                }
                EncodedInstruction::Union => {
                    let b = self.state.pop()?;
                    let a = self.state.pop()?;
                    let combined = a.union(
                        b,
                        self.runnable
                            .documents()
                            .borrow()
                            .document_order_access(self.xot()),
                    )?;
                    self.state.push(combined);
                }
                EncodedInstruction::Intersect => {
                    let b = self.state.pop()?;
                    let a = self.state.pop()?;
                    let combined = a.intersect(
                        b,
                        self.runnable
                            .documents()
                            .borrow()
                            .document_order_access(self.xot()),
                    )?;
                    self.state.push(combined);
                }
                EncodedInstruction::Except => {
                    let b = self.state.pop()?;
                    let a = self.state.pop()?;
                    let combined = a.except(
                        b,
                        self.runnable
                            .documents()
                            .borrow()
                            .document_order_access(self.xot()),
                    )?;
                    self.state.push(combined);
                }
                EncodedInstruction::Dup => {
                    let value = self.state.pop()?;
                    self.state.push(value.clone());
                    self.state.push(value);
                }
                EncodedInstruction::Pop => {
                    self.state.pop()?;
                }
                EncodedInstruction::Call => {
                    let arity = self.read_u8();
                    self.call(arity)?;
                }
                EncodedInstruction::Lookup => {
                    self.lookup()?;
                }
                EncodedInstruction::WildcardLookup => {
                    self.wildcard_lookup()?;
                }
                EncodedInstruction::Step => {
                    let step_id = self.read_u16();
                    let node: xot::Node = self.state.pop()?.try_into()?;
                    // Get step data before mutably borrowing self.state.xot.
                    // Clone the Rc<Program> (cheap ref-count bump) or use
                    // self.runnable.program() to avoid borrow conflict.
                    let function_id = self.state.frame().function();
                    let owned_program = self.state.frame().owned_program().cloned();
                    let program = owned_program
                        .as_deref()
                        .unwrap_or_else(|| self.runnable.program());
                    let step = &program.inline_function(function_id).steps[step_id as usize];
                    let value = xml::resolve_step(
                        step,
                        node,
                        self.state.xot,
                        &mut self.state.namespace_parents,
                    );
                    self.state.push(value);
                }
                EncodedInstruction::Deduplicate => {
                    let value = self.state.pop()?;
                    let value = value.deduplicate(
                        self.runnable
                            .documents()
                            .borrow()
                            .document_order_access(self.xot()),
                    )?;
                    self.state.push(value);
                }
                EncodedInstruction::Return => {
                    // Pop empty regex groups pushed when entering a stylesheet function
                    if self.current_inline_function().declared_name.is_some() {
                        self.state.pop_regex_groups();
                    }
                    self.runnable.dynamic_context.pop_static_base_uri();
                    if self.state.inline_return(start_base) {
                        break;
                    }
                }
                EncodedInstruction::ReturnConvert => {
                    let sequence_type_id = self.read_u16();
                    let sequence = self.state.pop()?;
                    let sequence_type =
                        &(self.current_inline_function().sequence_types[sequence_type_id as usize]);

                    let sequence = sequence.sequence_type_matching_function_conversion(
                        sequence_type,
                        self.runnable.static_context(),
                        self.state.xot(),
                        &|function| function.signature(self.runnable.program()).clone(),
                    )?;
                    self.state.push(sequence);
                }
                EncodedInstruction::ConvertSequence => {
                    let sequence_type_id = self.read_u16();
                    let raised_error = RaisedError::from_u16(self.read_u16());
                    let sequence = self.state.pop()?;
                    let sequence_type =
                        &(self.current_inline_function().sequence_types[sequence_type_id as usize]);

                    let sequence = sequence
                        .sequence_type_matching_function_conversion(
                            sequence_type,
                            self.runnable.static_context(),
                            self.state.xot(),
                            &|function| function.signature(self.runnable.program()).clone(),
                        )
                        .map_err(|e| {
                            let detail = e.detail().map(|s| s.to_string());
                            match raised_error {
                                RaisedError::XTTE0590 => {
                                    if std::env::var("XEE_WATCH_STACK").is_ok() {
                                        eprintln!("[WATCH] XTTE0590 triggered!");
                                        eprintln!("[WATCH]   fn={}, base={}", self.state.frame().function().get(), self.state.frame().base());
                                        eprintln!("[WATCH]   sequence_type: {:?}", sequence_type);
                                        eprintln!("[WATCH]   detail: {:?}", detail);
                                        eprintln!("[WATCH]   stack_len: {}", self.state.stack().len());
                                        eprintln!("[WATCH]   frames:");
                                        for (i, f) in self.state.frames_debug().iter().enumerate() {
                                            eprintln!("[WATCH]     [{}] fn={} base={}", i, f.function().get(), f.base());
                                        }
                                        eprintln!("[WATCH]   stack contents:");
                                        for (i, v) in self.state.stack().iter().enumerate() {
                                            let summary = match v {
                                                stack::Value::Absent => "Absent".to_string(),
                                                stack::Value::Sequence(s) => format!("{} items: {:?}", s.len(), s),
                                            };
                                            let summary = if summary.len() > 120 { format!("{}...", &summary[..120]) } else { summary };
                                            eprintln!("[WATCH]     [{}] {}", i, summary);
                                        }
                                    }
                                    error::Error::XTTE0590(detail)
                                },
                                RaisedError::XTDE0560 => error::Error::XTDE0560,
                                RaisedError::XTDE0700 => error::Error::XTDE0700,
                                RaisedError::XTDE1425 => error::Error::XTDE1425,
                                RaisedError::XTTE0570 => error::Error::XTTE0570,
                                RaisedError::XTMM9000 => error::Error::XTMM9000,
                                RaisedError::XPTY0004 => error::Error::XPTY0004(detail),
                            }
                        })?;
                    self.state.push(sequence);
                }
                EncodedInstruction::LetDone => {
                    let return_value = self.state.pop()?;
                    // pop the variable assignment
                    let _ = self.state.pop();
                    self.state.push(return_value);
                }
                EncodedInstruction::Cast => {
                    let type_id = self.read_u16();
                    let value = self.pop_atomic_option()?;
                    let cast_type = &(self.current_inline_function().cast_types[type_id as usize]);
                    if let Some(value) = value {
                        let cast_value = value
                            .cast_to_schema_type(cast_type.xs, self.runnable.static_context())?;
                        self.state.push(cast_value);
                    } else if cast_type.empty_sequence_allowed {
                        self.state.push(sequence::Sequence::default());
                    } else {
                        Err(error::Error::type_error(format!(
                            "cast requires a value, got empty sequence (target type: {:?})",
                            cast_type.xs
                        )))?;
                    }
                }
                EncodedInstruction::Castable => {
                    let type_id = self.read_u16();
                    let value = self.pop_atomic_option()?;
                    let cast_type = &(self.current_inline_function().cast_types[type_id as usize]);
                    if let Some(value) = value {
                        let cast_value =
                            value.cast_to_schema_type(cast_type.xs, self.runnable.static_context());
                        self.state.push(cast_value.is_ok());
                    } else if cast_type.empty_sequence_allowed {
                        self.state.push(true)
                    } else {
                        self.state.push(false);
                    }
                }
                EncodedInstruction::InstanceOf => {
                    let sequence_type_id = self.read_u16();
                    let sequence = self.state.pop()?;
                    let sequence_type =
                        &(self.current_inline_function().sequence_types[sequence_type_id as usize]);
                    let matches = sequence.sequence_type_matching(
                        sequence_type,
                        self.state.xot(),
                        &|function| function.signature(self.runnable.program()).clone(),
                    );
                    if matches.is_ok() {
                        self.state.push(true);
                    } else {
                        self.state.push(false);
                    }
                }
                EncodedInstruction::Treat => {
                    let sequence_type_id = self.read_u16();
                    let sequence = self.state.top()?;
                    let sequence_type =
                        &(self.current_inline_function().sequence_types[sequence_type_id as usize]);
                    let matches = sequence.sequence_type_matching(
                        sequence_type,
                        self.state.xot(),
                        &|function| function.signature(self.runnable.program()).clone(),
                    );
                    if matches.is_err() {
                        Err(error::Error::XPDY0050)?;
                    }
                }
                EncodedInstruction::Range => {
                    let b = self.state.pop()?;
                    let a = self.state.pop()?;
                    let a = a.atomized_option(self.state.xot())?;
                    let b = b.atomized_option(self.state.xot())?;
                    let (a, b) = match (a, b) {
                        (None, None) | (None, _) | (_, None) => {
                            self.state.push(sequence::Sequence::default());
                            continue;
                        }
                        (Some(a), Some(b)) => (a, b),
                    };
                    // we want to ensure we have integers at this point;
                    // we don't want to be casting strings or anything
                    a.ensure_base_schema_type(Xs::Integer)?;
                    b.ensure_base_schema_type(Xs::Integer)?;

                    let a: IBig = a.try_into().unwrap();
                    let b: IBig = b.try_into().unwrap();

                    match a.cmp(&b) {
                        Ordering::Greater => self.state.push(sequence::Sequence::default()),
                        Ordering::Equal => self.state.push(a),
                        Ordering::Less => {
                            let sequence: sequence::Sequence =
                                sequence::Range::new(a, b + 1)?.into();
                            self.state.push(sequence)
                        }
                    }
                }

                EncodedInstruction::SequenceLen => {
                    let value = self.state.pop()?;
                    let l: IBig = value.len().into();
                    self.state.push(l);
                }
                EncodedInstruction::SequenceGet => {
                    let value = self.state.pop()?;
                    let index = self.pop_atomic()?;
                    let index = index.cast_to_integer_value::<i64>()? as usize;
                    // substract 1 as Xpath is 1-indexed
                    let item = value
                        .get(index - 1)
                        .ok_or(error::Error::type_error(format!(
                            "sequence index {} out of bounds (length {})",
                            index,
                            value.len()
                        )))?;
                    let sequence: sequence::Sequence = item.into();
                    self.state.push(sequence)
                }
                EncodedInstruction::BuildNew => {
                    self.state.build_new();
                }
                EncodedInstruction::BuildPush => {
                    self.state.build_push()?;
                }
                EncodedInstruction::BuildComplete => {
                    self.state.build_complete();
                }
                EncodedInstruction::IsNumeric => {
                    let is_numeric = self.pop_is_numeric()?;
                    self.state.push(is_numeric);
                }
                EncodedInstruction::XmlName => {
                    let local_name_value = self.pop_atomic()?;
                    let namespace_value = self.pop_atomic()?;
                    let namespace = namespace_value.to_str()?;
                    let local_name = local_name_value.to_string()?;
                    let name =
                        xee_name::Name::new(local_name, namespace.to_string(), String::new());
                    self.state.push(name);
                }
                EncodedInstruction::XmlDocument => {
                    let root_node = self.state.xot.new_document();
                    let item = sequence::Item::Node(root_node);
                    self.state.push(item);
                }
                EncodedInstruction::XmlElement => {
                    let name_id = self.pop_xot_name()?;
                    let element_node = self.state.xot.new_element(name_id);
                    let item = sequence::Item::Node(element_node);
                    self.state.push(item);
                }
                EncodedInstruction::XmlAttribute => {
                    let value = self.pop_atomic()?;
                    let name_id = self.pop_xot_name()?;
                    let attribute_node = self
                        .state
                        .xot
                        .new_attribute_node(name_id, value.string_value());
                    let item = sequence::Item::Node(attribute_node);
                    self.state.push(item);
                }
                EncodedInstruction::XmlNamespace => {
                    let uri = self.pop_atomic()?;
                    let namespace_id = self.state.xot.add_namespace(&uri.string_value());
                    let prefix = self.pop_atomic()?;
                    let prefix_id = self.state.xot.add_prefix(&prefix.string_value());
                    let namespace_node = self.state.xot.new_namespace_node(prefix_id, namespace_id);
                    let item = sequence::Item::Node(namespace_node);
                    self.state.push(item);
                }
                EncodedInstruction::XmlText => {
                    let text_atomic = self.pop_atomic()?;
                    let text = text_atomic.into_canonical();
                    let text_node = self.state.xot.new_text(&text);
                    let item = sequence::Item::Node(text_node);
                    self.state.push(item);
                }
                EncodedInstruction::XmlComment => {
                    let text_atomic = self.pop_atomic()?;
                    let text = text_atomic.into_canonical();
                    let comment_node = self.state.xot.new_comment(&text);
                    let item = sequence::Item::Node(comment_node);
                    self.state.push(item);
                }
                EncodedInstruction::XmlProcessingInstruction => {
                    let text_atomic = self.pop_atomic()?;
                    let text = text_atomic.into_canonical();
                    let text = if !text.is_empty() {
                        Some(text.as_str())
                    } else {
                        None
                    };
                    let target_atomic = self.pop_atomic()?;
                    let target = target_atomic.into_canonical();
                    let target_id = self.state.xot.add_name(&target);
                    let pi_node = self.state.xot.new_processing_instruction(target_id, text);
                    let item = sequence::Item::Node(pi_node);
                    self.state.push(item);
                }
                EncodedInstruction::XmlAppend => {
                    let child_value = self.state.pop()?;
                    let parent_node = self.pop_node()?;
                    self.xml_append(parent_node, child_value)?;
                    // now we can push back the parent node
                    let item = sequence::Item::Node(parent_node);
                    self.state.push(item);
                }
                EncodedInstruction::CopyShallow => {
                    let value = &self.state.pop()?;
                    if value.is_empty() {
                        self.state.push(sequence::Sequence::default());
                        continue;
                    }
                    if value.len() > 1 {
                        Err(error::Error::XTTE3180)?;
                    }
                    let item = value.iter().next().unwrap();
                    let copy = match &item {
                        sequence::Item::Atomic(_) | sequence::Item::Function(_) => item.clone(),
                        sequence::Item::Node(node) => {
                            let copied_node = self.shallow_copy_node(*node);
                            sequence::Item::Node(copied_node)
                        }
                    };
                    self.state.push(copy);
                }
                EncodedInstruction::CopyDeep => {
                    let value = &self.state.pop()?;
                    if value.is_empty() {
                        self.state.push(sequence::Sequence::default());
                        continue;
                    }
                    let mut new_sequence = Vec::with_capacity(value.len());
                    for item in value.iter() {
                        let copy = match &item {
                            sequence::Item::Atomic(_) | sequence::Item::Function(_) => item.clone(),
                            sequence::Item::Node(node) => {
                                let copied_node = self.state.xot.clone_with_prefixes(*node);
                                sequence::Item::Node(copied_node)
                            }
                        };
                        new_sequence.push(copy);
                    }
                    self.state.push(new_sequence);
                }
                EncodedInstruction::CallTemplate => {
                    let tunnel_params = self.state.pop()?.one()?.to_map()?;
                    let params = self.state.pop()?.one()?.to_map()?;
                    let size = self.pop_optional_sequence();
                    let position = self.pop_optional_sequence();
                    let item = self.pop_optional_sequence();
                    let function = self.state.pop()?.one()?.to_function()?;
                    self.push_error_context("calling template");
                    let value = self.call_template_with_params(
                        &function,
                        [item, position, size],
                        &params,
                        &tunnel_params,
                    );
                    match value {
                        Ok(v) => {
                            self.pop_error_context();
                            self.state.push(v);
                        }
                        Err(e) => return Err(e),
                    }
                }
                EncodedInstruction::ApplyTemplates => {
                    let tunnel_params = self.state.pop()?.one()?.to_map()?;
                    let params = self.state.pop()?.one()?.to_map()?;
                    let value = self.state.pop()?;
                    let mode_id = self.read_u16();
                    let builtin_template_params_passthrough = self.read_u8() != 0;
                    let mode = pattern::ModeId::new(mode_id as usize);
                    self.push_error_context("applying templates");
                    let value = self.apply_templates_sequence(
                        mode,
                        value,
                        &params,
                        &tunnel_params,
                        builtin_template_params_passthrough,
                    );
                    match value {
                        Ok(v) => {
                            self.pop_error_context();
                            self.state.push(v);
                        }
                        Err(e) => return Err(e),
                    }
                }
                EncodedInstruction::ApplyTemplatesCurrent => {
                    let tunnel_params = self.state.pop()?.one()?.to_map()?;
                    let params = self.state.pop()?.one()?.to_map()?;
                    let value = self.state.pop()?;
                    let fallback_mode_id = self.read_u16();
                    let builtin_template_params_passthrough = self.read_u8() != 0;
                    let mode = self
                        .current_mode_or_fallback(pattern::ModeId::new(fallback_mode_id as usize));
                    self.push_error_context("applying templates");
                    let value = self.apply_templates_sequence(
                        mode,
                        value,
                        &params,
                        &tunnel_params,
                        builtin_template_params_passthrough,
                    );
                    match value {
                        Ok(v) => {
                            self.pop_error_context();
                            self.state.push(v);
                        }
                        Err(e) => return Err(e),
                    }
                }
                EncodedInstruction::ContinueTemplate => {
                    let tunnel_params = self.state.pop()?.one()?.to_map()?;
                    let params = self.state.pop()?.one()?.to_map()?;
                    let behavior = self.read_u8();
                    let value =
                        self.continue_template_with_params(&params, &tunnel_params, behavior)?;
                    self.state.push(value);
                }
                EncodedInstruction::RaiseError => {
                    let error = match RaisedError::from_u16(self.read_u16()) {
                        RaisedError::XTDE0560 => error::Error::XTDE0560,
                        RaisedError::XTDE0700 => error::Error::XTDE0700,
                        RaisedError::XTDE1425 => error::Error::XTDE1425,
                        RaisedError::XTTE0570 => error::Error::XTTE0570,
                        RaisedError::XTTE0590 => error::Error::XTTE0590(None),
                        RaisedError::XTMM9000 => error::Error::XTMM9000,
                        RaisedError::XPTY0004 => error::Error::XPTY0004(None),
                    };
                    return Err(error);
                }
                EncodedInstruction::PrintTop => {
                    let top = self.state.top()?;
                    println!("{:#?}", top);
                }
                EncodedInstruction::PrintStack => {
                    println!("{:#?}", self.state.stack());
                }
            }
        }
        Ok(())
    }

    pub(crate) fn create_static_closure_from_stack(
        &mut self,
        static_function_id: function::StaticFunctionId,
    ) -> error::Result<function::Function> {
        Self::create_static_closure(self.runnable.dynamic_context(), static_function_id, || {
            Some(self.state.pop_value())
        })
    }

    pub(crate) fn create_static_closure_from_context(
        &mut self,
        static_function_id: function::StaticFunctionId,
        arg: Option<xot::Node>,
    ) -> error::Result<function::Function> {
        Self::create_static_closure(self.runnable.dynamic_context(), static_function_id, || {
            arg.map(|n| {
                let value: stack::Value = n.into();
                value
            })
        })
    }

    pub(crate) fn create_static_closure<F>(
        context: &DynamicContext,
        static_function_id: function::StaticFunctionId,
        mut get: F,
    ) -> error::Result<function::Function>
    where
        F: FnMut() -> Option<stack::Value>,
    {
        let static_function = &context.static_context().function_by_id(static_function_id);
        // get any context value from the stack if needed
        let closure_vars = if static_function.needs_context() {
            let value = get();
            if let Some(value) = value {
                vec![value]
            } else {
                vec![]
            }
        } else {
            vec![]
        };
        Ok(function::StaticFunctionData::new(static_function_id, closure_vars).into())
    }

    pub(crate) fn current_inline_function(&self) -> &function::InlineFunction {
        self.current_program().inline_function(self.state.frame().function())
    }

    fn current_program(&self) -> &super::Program {
        self.state
            .frame()
            .owned_program()
            .map(Rc::as_ref)
            .unwrap_or_else(|| self.runnable.program())
    }

    fn current_owned_program(&self) -> Option<&Rc<super::Program>> {
        self.state.frame().owned_program()
    }

    fn inline_function_value(
        &self,
        function_id: function::InlineFunctionId,
        closure_vars: Vec<stack::Value>,
    ) -> function::Function {
        match self.current_owned_program() {
            Some(program) => {
                function::InlineFunctionData::new_with_program(
                    Rc::clone(program),
                    function_id,
                    closure_vars,
                )
                .into()
            }
            None => function::InlineFunctionData::new(function_id, closure_vars).into(),
        }
    }

    pub(crate) fn resolve_global_variable(
        &mut self,
        index: usize,
    ) -> error::Result<sequence::Sequence> {
        match &self.global_variables[index] {
            GlobalValueState::Resolved(value) => Ok(value.clone()),
            GlobalValueState::Resolving => Err(error::Error::XTDE0640),
            GlobalValueState::Uninitialized => {
                let global = self.current_program().declarations.global_variable(index).clone();
                if global.external {
                    let Some(original_name) = &global.original_name else {
                        return Err(error::Error::Unsupported(
                            "External global declaration missing original name".to_string(),
                        ));
                    };
                    if let Some(value) = self
                        .runnable
                        .dynamic_context()
                        .variables()
                        .get(original_name)
                    {
                        let value = value.clone();
                        self.global_variables[index] = GlobalValueState::Resolved(value.clone());
                        return Ok(value);
                    }
                    if global.required {
                        return Err(error::Error::XTDE0050);
                    }
                }

                self.global_variables[index] = GlobalValueState::Resolving;
                let function = self.inline_function_value(global.function_id, Vec::new());
                let context_arguments =
                    if let Some(context_item) = self.runnable.dynamic_context().context_item() {
                        [
                            Some(context_item.clone().into()),
                            Some(ibig::ibig!(1).into()),
                            Some(ibig::ibig!(1).into()),
                        ]
                    } else {
                        [None, None, None]
                    };
                let value =
                    self.call_function_with_optional_arguments(&function, context_arguments.into())?;
                self.global_variables[index] = GlobalValueState::Resolved(value.clone());
                Ok(value)
            }
        }
    }

    pub(crate) fn has_resolving_global_variable(&self) -> bool {
        self.global_variables
            .iter()
            .any(|state| matches!(state, GlobalValueState::Resolving))
    }

    pub(crate) fn resolving_global_variable_count(&self) -> usize {
        self.global_variables
            .iter()
            .filter(|state| matches!(state, GlobalValueState::Resolving))
            .count()
    }

    pub(crate) fn function_name(&self, function: &function::Function) -> Option<Name> {
        function.name(self.runnable.program())
    }

    pub(crate) fn function_arity(&self, function: &function::Function) -> usize {
        function.arity(self.runnable.program())
    }

    fn call(&mut self, arity: u8) -> error::Result<()> {
        let function = self.state.callable(arity as usize)?;
        self.call_function(&function, arity)
    }

    pub(crate) fn call_function_with_arguments(
        &mut self,
        function: &function::Function,
        arguments: Vec<sequence::Sequence>,
    ) -> error::Result<sequence::Sequence> {
        let arguments = arguments.into_iter().map(Some).collect::<Vec<_>>();
        self.call_function_with_optional_arguments(function, arguments)
    }

    pub(crate) fn call_function_with_arguments_catching(
        &mut self,
        function: &function::Function,
        arguments: Vec<sequence::Sequence>,
    ) -> error::Result<sequence::Sequence> {
        let checkpoint = self.state.checkpoint();
        let tunnel_params_len = self.tunnel_params.len();
        let mode_stack_len = self.mode_stack.len();
        let template_rule_stack_len = self.template_rule_stack.len();

        match self.call_function_with_arguments(function, arguments) {
            Ok(result) => Ok(result),
            Err(error) => {
                self.state.restore(checkpoint);
                self.tunnel_params.truncate(tunnel_params_len);
                self.mode_stack.truncate(mode_stack_len);
                self.template_rule_stack.truncate(template_rule_stack_len);
                Err(error)
            }
        }
    }

    pub(crate) fn call_function_with_arguments_catching_spanned(
        &mut self,
        function: &function::Function,
        arguments: Vec<sequence::Sequence>,
    ) -> error::SpannedResult<sequence::Sequence> {
        let checkpoint = self.state.checkpoint();
        let tunnel_params_len = self.tunnel_params.len();
        let mode_stack_len = self.mode_stack.len();
        let template_rule_stack_len = self.template_rule_stack.len();

        match self.call_function_with_arguments_spanned(function, arguments) {
            Ok(result) => Ok(result),
            Err(error) => {
                self.state.restore(checkpoint);
                self.tunnel_params.truncate(tunnel_params_len);
                self.mode_stack.truncate(mode_stack_len);
                self.template_rule_stack.truncate(template_rule_stack_len);
                Err(error)
            }
        }
    }

    pub(crate) fn call_function_with_arguments_catching_spanned_with_rollback(
        &mut self,
        function: &function::Function,
        arguments: Vec<sequence::Sequence>,
        rollback_output: bool,
    ) -> error::SpannedResult<sequence::Sequence> {
        let checkpoint = self.state.checkpoint();
        let tunnel_params_len = self.tunnel_params.len();
        let mode_stack_len = self.mode_stack.len();
        let template_rule_stack_len = self.template_rule_stack.len();

        match self.call_function_with_arguments_spanned(function, arguments) {
            Ok(result) => Ok(result),
            Err(error) => {
                let output_changed = self.state.output_changed_since(&checkpoint);
                self.state.restore(checkpoint);
                self.tunnel_params.truncate(tunnel_params_len);
                self.mode_stack.truncate(mode_stack_len);
                self.template_rule_stack.truncate(template_rule_stack_len);
                if !rollback_output && output_changed {
                    Err(error::SpannedError {
                        error: error::Error::XTDE3530,
                        span: error.span,
                        detail: None,
                        contexts: error.contexts,
                    })
                } else {
                    Err(error)
                }
            }
        }
    }

    pub(crate) fn call_function_with_arguments_spanned(
        &mut self,
        function: &function::Function,
        arguments: Vec<sequence::Sequence>,
    ) -> error::SpannedResult<sequence::Sequence> {
        let arguments = arguments.into_iter().map(Some).collect::<Vec<_>>();
        self.call_function_with_optional_arguments_spanned(function, arguments)
    }

    pub(crate) fn call_function_with_optional_arguments(
        &mut self,
        function: &function::Function,
        arguments: Vec<Option<sequence::Sequence>>,
    ) -> error::Result<sequence::Sequence> {
        // put function onto the stack
        let item: sequence::Item = function.clone().into();
        self.state.push(item);
        // then arguments — move into stack, no cloning
        let arity = arguments.len() as u8;
        for arg in arguments {
            if let Some(arg) = arg {
                self.state.push(arg);
            } else {
                self.state.push_value(stack::Value::Absent);
            }
        }
        self.call_function(function, arity)?;
        if matches!(function, function::Function::Inline(_)) {
            // run interpreter until we return to the base
            // we started in
            self.run_actual(self.state.frame().base())?;
        }
        self.state.pop()
    }

    pub(crate) fn call_function_with_optional_arguments_spanned(
        &mut self,
        function: &function::Function,
        arguments: Vec<Option<sequence::Sequence>>,
    ) -> error::SpannedResult<sequence::Sequence> {
        let item: sequence::Item = function.clone().into();
        self.state.push(item);
        let arity = arguments.len() as u8;
        for arg in arguments {
            if let Some(arg) = arg {
                self.state.push(arg);
            } else {
                self.state.push_value(stack::Value::Absent);
            }
        }
        self.call_function(function, arity)
            .map_err(|error| self.err(error))?;
        if matches!(function, function::Function::Inline(_)) {
            self.run(self.state.frame().base())?;
        }
        self.state.pop().map_err(|error| self.err(error))
    }

    fn call_function(&mut self, function: &function::Function, arity: u8) -> error::Result<()> {
        match function {
            function::Function::Static(data) => {
                self.call_static(data.id, arity, &data.closure_vars)
            }
            function::Function::Inline(data) => self.call_inline(data, arity),
            function::Function::Coerced(data) => self.call_coerced(data, arity),
            function::Function::Concat(data) => self.call_concat(data, arity),
            function::Function::PatternMatcher(data) => self.call_pattern_matcher(data, arity),
            function::Function::Array(array) => self.call_array(array, arity as usize),
            function::Function::Map(map) => self.call_map(map, arity as usize),
        }
    }

    pub(crate) fn arguments(&self, arity: u8) -> &[stack::Value] {
        self.state.arguments(arity as usize)
    }

    fn call_static(
        &mut self,
        static_function_id: function::StaticFunctionId,
        arity: u8,
        closure_vars: &[stack::Value],
    ) -> error::Result<()> {
        let static_function = self.current_program().static_function(static_function_id).clone();
        if arity as usize != static_function.arity() {
            return Err(error::Error::type_error(format!(
                "function expects {} argument(s), got {}",
                static_function.arity(),
                arity
            )));
        }
        let parameter_types = static_function.signature().parameter_types().to_vec();
        let arguments = self.coerce_arguments(&parameter_types, arity)?;
        let result =
            static_function.invoke(self.runnable.dynamic_context, self, arguments, closure_vars)?;
        // pop the last item off
        let _ = self.state.pop();
        self.state.push(result);
        Ok(())
    }

    fn call_inline(
        &mut self,
        function: &function::InlineFunctionData,
        arity: u8,
    ) -> error::Result<()> {
        // look up the function in order to access the parameters information
        let program = function.program.as_deref().unwrap_or_else(|| self.runnable.program());
        let inline_function = program.inline_function(function.id);
        let parameter_types = &inline_function.signature.parameter_types();
        if arity as usize != parameter_types.len() {
            return Err(error::Error::type_error(format!(
                "function expects {} argument(s), got {}",
                parameter_types.len(),
                arity
            )));
        }

        // Push per-function static base URI for correct relative URI resolution
        self.runnable
            .dynamic_context
            .push_static_base_uri(inline_function.static_base_uri.clone());

        let arguments = self.coerce_inline_arguments(parameter_types, arity)?;

        // now we have a list of arguments that we want to push back onto the stack
        // (they are already reversed)
        for arg in arguments {
            self.state.push_value(arg);
        }

        // Stylesheet functions (xsl:function) isolate the regex-group context.
        // Push an empty set so regex-group() returns empty inside the function,
        // while any inner xsl:analyze-string can push its own groups on top.
        if inline_function.declared_name.is_some() {
            self.state.push_regex_groups(vec![]);
        }

        self.state
            .push_frame(function.id, arity as usize, function.program.clone())
    }

    fn call_coerced(
        &mut self,
        function: &function::CoercedFunctionData,
        arity: u8,
    ) -> error::Result<()> {
        if arity as usize != function.signature.arity() {
            return Err(error::Error::type_error(format!(
                "function expects {} argument(s), got {}",
                function.signature.arity(),
                arity
            )));
        }

        let arguments = self.coerce_arguments(function.signature.parameter_types(), arity)?;
        let result = self.call_function_with_arguments_catching(&function.function, arguments)?;
        let result = if let Some(return_type) = function.signature.return_type() {
            result.sequence_type_matching_function_conversion(
                return_type,
                self.runnable.static_context(),
                self.state.xot(),
                &|wrapped_function| wrapped_function.signature(self.runnable.program()).clone(),
            )?
        } else {
            result
        };

        let _ = self.state.pop();
        self.state.push(result);
        Ok(())
    }

    fn call_concat(
        &mut self,
        function: &function::ConcatFunctionData,
        arity: u8,
    ) -> error::Result<()> {
        if arity as usize != function.arity {
            return Err(error::Error::type_error(format!(
                "function expects {} argument(s), got {}",
                function.arity,
                arity
            )));
        }

        let arguments = self.state.arguments(arity as usize);
        let strings = arguments
            .iter()
            .map(|value| {
                let sequence: sequence::Sequence = value.try_into()?;
                let atomic = sequence.atomized_option(self.state.xot())?;
                Ok(if let Some(atomic) = atomic {
                    atomic.string_value()
                } else {
                    String::new()
                })
            })
            .collect::<error::Result<Vec<String>>>()?;

        self.state.truncate_arguments(arity as usize);
        let _ = self.state.pop();
        self.state.push(strings.concat());
        Ok(())
    }

    fn call_pattern_matcher(
        &mut self,
        function: &function::PatternMatcherFunctionData,
        arity: u8,
    ) -> error::Result<()> {
        if arity != 1 {
            return Err(error::Error::type_error(format!(
                "function expects 1 argument, got {}",
                arity
            )));
        }

        let argument: sequence::Sequence = self.state.arguments(1)[0].clone().try_into()?;
        let items = argument.iter().collect::<Vec<_>>();
        if items.len() != 1 {
            return Err(error::Error::type_error(
                "pattern matcher expects exactly one item",
            ));
        }

        let matched = self.matches(&function.pattern, &items[0]);
        self.state.truncate_arguments(1);
        let _ = self.state.pop();
        self.state.push(matched);
        Ok(())
    }

    fn coerce_inline_arguments(
        &mut self,
        parameter_types: &[Option<ast::SequenceType>],
        arity: u8,
    ) -> error::Result<Vec<stack::Value>> {
        let stack_values = self.state.arguments(arity as usize);
        let mut arguments = Vec::with_capacity(arity as usize);
        let static_context = self.runnable.static_context();
        let xot = self.state.xot();
        for (parameter_type, stack_value) in parameter_types.iter().zip(stack_values) {
            if stack_value.is_absent() {
                arguments.push(stack::Value::Absent);
                continue;
            }
            let sequence: sequence::Sequence = stack_value.try_into()?;
            let value = if let Some(type_) = parameter_type {
                sequence
                    .sequence_type_matching_function_conversion(
                        type_,
                        static_context,
                        xot,
                        &|function| function.signature(self.runnable.program()).clone(),
                    )?
                    .into()
            } else {
                sequence.into()
            };
            arguments.push(value);
        }
        self.state.truncate_arguments(arity as usize);
        Ok(arguments)
    }

    fn coerce_arguments(
        &mut self,
        parameter_types: &[Option<ast::SequenceType>],
        arity: u8,
    ) -> error::Result<Vec<sequence::Sequence>> {
        // TODO: fast path if no sequence type declarations exist for
        // parameters could cache this inside of signature so that it's really
        // fast to detect.

        // we could also have a secondary fast path where if the types are all
        // exactly the same, we don't do a clone.

        // get all the stack values out in order
        let stack_values = self.state.arguments(arity as usize);
        let mut arguments = Vec::with_capacity(arity as usize);
        let static_context = self.runnable.static_context();
        let xot = self.state.xot();
        for (parameter_type, stack_value) in parameter_types.iter().zip(stack_values) {
            let sequence: sequence::Sequence = stack_value.try_into()?;
            if let Some(type_) = parameter_type {
                // matching also takes care of function conversion rules
                let sequence = sequence.sequence_type_matching_function_conversion(
                    type_,
                    static_context,
                    xot,
                    &|function| function.signature(self.runnable.program()).clone(),
                )?;
                arguments.push(sequence);
            } else {
                // no need to do any checking or conversion
                arguments.push(sequence);
            }
        }
        self.state.truncate_arguments(arity as usize);
        Ok(arguments)
    }

    fn call_array(&mut self, array: &function::Array, arity: usize) -> error::Result<()> {
        if arity != 1 {
            return Err(error::Error::type_error(format!(
                "array lookup expects 1 argument, got {}",
                arity
            )));
        }
        // the argument
        let position = self.pop_atomic()?;
        let sequence = Self::array_get(array, position)?;
        // pop the array off the stack
        self.state.pop()?;
        // now push the result
        self.state.push(sequence);
        Ok(())
    }

    fn array_get(
        array: &function::Array,
        position: atomic::Atomic,
    ) -> error::Result<sequence::Sequence> {
        let position = position
            .cast_to_integer_value::<i64>()
            .map_err(|_| error::Error::type_error("array index must be an integer"))?;
        let position = position as usize;
        if position == 0 {
            return Err(error::Error::FOAY0001);
        }
        let position = position - 1;
        let sequence = array.index(position);
        sequence.cloned().ok_or(error::Error::FOAY0001)
    }

    fn call_map(&mut self, map: &function::Map, arity: usize) -> error::Result<()> {
        if arity != 1 {
            return Err(error::Error::type_error(format!(
                "map lookup expects 1 argument, got {}",
                arity
            )));
        }
        let key = self.pop_atomic()?;
        let value = map.get(&key);
        // pop the map off the stack
        self.state.pop()?;
        if let Some(value) = value {
            self.state.push(value.clone());
        } else {
            self.state.push(sequence::Sequence::default());
        }
        Ok(())
    }

    fn lookup(&mut self) -> error::Result<()> {
        let key_specifier = self.state.pop()?;
        let value = self.state.pop()?;
        let function: function::Function = value.try_into()?;
        let value = self.lookup_value(&function, key_specifier)?;
        let sequence: sequence::Sequence = value.into();
        self.state.push(sequence);
        Ok(())
    }

    fn lookup_value(
        &self,
        function: &function::Function,
        key_specifier: sequence::Sequence,
    ) -> error::Result<Vec<sequence::Item>> {
        match function {
            function::Function::Map(map) => self.lookup_map(map, key_specifier),
            function::Function::Array(array) => self.lookup_array(array, key_specifier),
            _ => Err(error::Error::type_error("lookup requires a map or array")),
        }
    }

    fn lookup_map(
        &self,
        map: &function::Map,
        key_specifier: sequence::Sequence,
    ) -> error::Result<Vec<sequence::Item>> {
        self.lookup_helper(key_specifier, map, |map, atomic| {
            Ok(map.get(&atomic).cloned().unwrap_or_default())
        })
    }

    fn lookup_array(
        &self,
        array: &function::Array,
        key_specifier: sequence::Sequence,
    ) -> error::Result<Vec<sequence::Item>> {
        self.lookup_helper(key_specifier, array, |array, atomic| match atomic {
            atomic::Atomic::Integer(..) => Self::array_get(array, atomic),
            _ => Err(error::Error::type_error(
                "array lookup key must be an integer",
            )),
        })
    }

    fn lookup_helper<T>(
        &self,
        key_specifier: sequence::Sequence,
        data: T,
        get_key: impl Fn(&T, atomic::Atomic) -> error::Result<sequence::Sequence>,
    ) -> error::Result<Vec<sequence::Item>> {
        let keys = key_specifier
            .atomized(self.state.xot())
            .collect::<error::Result<Vec<_>>>()?;
        let mut result = Vec::new();
        for key in keys {
            for item in get_key(&data, key)?.iter() {
                result.push(item.clone());
            }
        }
        Ok(result)
    }

    fn wildcard_lookup(&mut self) -> error::Result<()> {
        let value = self.state.pop()?;
        let function: function::Function = value.try_into()?;
        let value = match function {
            function::Function::Map(map) => {
                let mut result = Vec::new();
                for key in map.keys() {
                    for value in self.lookup_map(&map, key.clone().into())? {
                        result.push(value)
                    }
                }
                result
            }
            function::Function::Array(array) => {
                let mut result = Vec::new();
                for i in 1..(array.len() + 1) {
                    let i: IBig = i.into();
                    for value in self.lookup_array(&array, i.into())? {
                        result.push(value)
                    }
                }
                result
            }
            _ => {
                return Err(error::Error::type_error(
                    "wildcard lookup requires a map or array",
                ))
            }
        };
        let sequence: sequence::Sequence = value.into();
        self.state.push(sequence);
        Ok(())
    }

    fn value_compare<O>(&mut self, op: O) -> error::Result<()>
    where
        O: AtomicCompare,
    {
        let b = self.state.pop()?;
        let a = self.state.pop()?;
        // https://www.w3.org/TR/xpath-31/#id-value-comparisons
        // If an operand is the empty sequence, the result is the empty sequence
        if a.is_empty() || b.is_empty() {
            self.state.push(sequence::Sequence::default());
            return Ok(());
        }
        let v = a.value_compare(
            &b,
            op,
            self.runnable.default_collation()?.as_ref(),
            self.runnable.implicit_timezone(),
            self.state.xot(),
        )?;
        self.state.push(v);
        Ok(())
    }

    fn general_compare<O>(&mut self, op: O) -> error::Result<()>
    where
        O: AtomicCompare,
    {
        let b = self.state.pop()?;
        let a = self.state.pop()?;
        let value =
            a.general_comparison(&b, op, self.runnable.dynamic_context(), self.state.xot())?;
        self.state.push(value);
        Ok(())
    }

    fn arithmetic<F>(&mut self, op: F) -> error::Result<()>
    where
        F: Fn(atomic::Atomic, atomic::Atomic) -> error::Result<atomic::Atomic>,
    {
        self.arithmetic_with_offset(|a, b, _| op(a, b))
    }

    fn arithmetic_with_offset<F>(&mut self, op: F) -> error::Result<()>
    where
        F: Fn(atomic::Atomic, atomic::Atomic, chrono::FixedOffset) -> error::Result<atomic::Atomic>,
    {
        let b = self.state.pop()?;
        let a = self.state.pop()?;
        // https://www.w3.org/TR/xpath-31/#id-arithmetic
        // 2. If an operand is the empty sequence, the result is the empty sequence
        if a.is_empty() || b.is_empty() {
            self.state.push(sequence::Sequence::default());
            return Ok(());
        }
        let a = a.atomized_one(self.state.xot())?;
        let b = b.atomized_one(self.state.xot())?;
        let result = op(a, b, self.runnable.implicit_timezone())?;
        self.state.push(result);
        Ok(())
    }

    fn unary_arithmetic<F>(&mut self, op: F) -> error::Result<()>
    where
        F: Fn(atomic::Atomic) -> error::Result<atomic::Atomic>,
    {
        let a = self.state.pop()?;
        if a.is_empty() {
            self.state.push(sequence::Sequence::default());
            return Ok(());
        }
        let a = a.atomized_one(self.state.xot())?;
        let value = op(a)?;
        self.state.push(value);
        Ok(())
    }

    fn pop_is_numeric(&mut self) -> error::Result<bool> {
        let value = self.state.pop()?;
        // A multi-item sequence is never numeric (it's a general predicate value).
        // Only zero-or-one items can be numeric position predicates.
        if value.len() > 1 {
            return Ok(false);
        }
        let a = value.atomized_option(self.state.xot())?;
        if let Some(a) = a {
            Ok(a.is_numeric())
        } else {
            Ok(false)
        }
    }

    fn pop_atomic(&mut self) -> error::Result<atomic::Atomic> {
        let value = self.state.pop()?;
        value.atomized_one(self.state.xot())
    }

    fn pop_atomic_option(&mut self) -> error::Result<Option<atomic::Atomic>> {
        let value = self.state.pop()?;
        value.atomized_option(self.state.xot())
    }

    fn pop_xot_name(&mut self) -> error::Result<xot::NameId> {
        let value = self.pop_atomic()?;
        let name: xee_name::Name = value.try_into()?;
        let namespace = name.namespace();
        let ns = self.state.xot.add_namespace(namespace);
        Ok(self.state.xot.add_name_ns(name.local_name(), ns))
    }

    fn pop_node(&mut self) -> error::Result<xot::Node> {
        let value = self.state.pop()?;
        let node = value.one()?.to_node()?;
        Ok(node)
    }

    fn pop_atomic2(&mut self) -> error::Result<(atomic::Atomic, atomic::Atomic)> {
        let b = self.pop_atomic()?;
        let a = self.pop_atomic()?;
        Ok((a, b))
    }

    fn pop_atomic2_option(
        &mut self,
    ) -> error::Result<(Option<atomic::Atomic>, Option<atomic::Atomic>)> {
        let b = self.pop_atomic_option()?;
        let a = self.pop_atomic_option()?;
        Ok((a, b))
    }

    fn pop_effective_boolean(&mut self) -> error::Result<bool> {
        let a = self.state.pop()?;
        a.effective_boolean_value()
    }

    pub(crate) fn regex(&self, pattern: &str, flags: &str) -> error::Result<Rc<regexml::Regex>> {
        self.state.regex(pattern, flags)
    }

    pub fn xot(&self) -> &Xot {
        self.state.xot()
    }

    pub fn xot_mut(&mut self) -> &mut Xot {
        self.state.xot_mut()
    }

    pub(crate) fn push_regex_groups(&mut self, groups: Vec<String>) {
        self.state.push_regex_groups(groups);
    }

    pub(crate) fn pop_regex_groups(&mut self) {
        self.state.pop_regex_groups();
    }

    pub(crate) fn regex_group(&self, n: usize) -> String {
        self.state.regex_group(n)
    }

    pub(crate) fn push_current_group(
        &mut self,
        group: sequence::Sequence,
        key: Option<crate::atomic::Atomic>,
    ) {
        self.state.push_current_group(group, key);
    }

    pub(crate) fn pop_current_group(&mut self) {
        self.state.pop_current_group();
    }

    pub(crate) fn current_group(&self) -> sequence::Sequence {
        self.state.current_group()
    }

    pub(crate) fn current_grouping_key(&self) -> sequence::Sequence {
        self.state.current_grouping_key()
    }

    fn xml_append(
        &mut self,
        parent_node: xot::Node,
        value: sequence::Sequence,
    ) -> error::Result<()> {
        let mut string_values = Vec::new();
        self.xml_append_items(parent_node, &value, &mut string_values)?;
        // if there are any string values left in the end
        if !string_values.is_empty() {
            self.xml_append_string_values(parent_node, &string_values);
        }
        Ok(())
    }

    fn xml_append_items(
        &mut self,
        parent_node: xot::Node,
        value: &sequence::Sequence,
        string_values: &mut Vec<String>,
    ) -> error::Result<()> {
        // XTDE0410: track whether non-attribute/namespace children have been
        // added, either already present in the parent or during this call.
        let parent_is_element = self.state.xot.is_element(parent_node);
        let mut has_non_attr_child = if parent_is_element {
            self.state.xot.children(parent_node).any(|child| {
                !self.state.xot.is_attribute_node(child)
                    && !self.state.xot.is_namespace_node(child)
            })
        } else {
            false
        };

        for item in value.iter() {
            match item {
                sequence::Item::Node(node) => {
                    // if there were any string values before this node, add them
                    // to the node, separated by a space character
                    if !string_values.is_empty() {
                        self.xml_append_string_values(parent_node, &string_values);
                        string_values.clear();
                        has_non_attr_child = true;
                    }
                    match self.state.xot.value(node) {
                        xot::Value::Document => {
                            let children = self.state.xot.children(node).collect::<Vec<_>>();
                            for child in children {
                                let child = self.state.xot.clone_node(child);
                                self.state.xot.any_append(parent_node, child).unwrap();
                                self.state.record_output_mutation();
                                has_non_attr_child = true;
                            }
                            continue;
                        }
                        xot::Value::Text(text) => {
                            // zero length text nodes are skipped
                            // Can this even exist, or does Xot not have
                            // them anyway?
                            if text.get().is_empty() {
                                continue;
                            }
                        }
                        _ => {}
                    }

                    // if we have a parent we're already in another document,
                    // in which case we want to make a clone first
                    let node = if self.state.xot.parent(node).is_some() {
                        self.state.xot.clone_node(node)
                    } else {
                        node
                    };
                    let is_attr_or_ns = self.state.xot.is_attribute_node(node)
                        || self.state.xot.is_namespace_node(node);
                    // XTDE0420: namespace or attribute node on non-element
                    if is_attr_or_ns && !parent_is_element {
                        return Err(error::Error::XTDE0420);
                    }
                    // XTDE0410: attribute/namespace after non-attribute/namespace child
                    if is_attr_or_ns && has_non_attr_child {
                        return Err(error::Error::XTDE0410);
                    }
                    let needs_attr_ns_fixup = parent_is_element
                        && self.state.xot.is_attribute_node(node);
                    let is_element = self.state.xot.is_element(node);
                    self.state.xot.any_append(parent_node, node).map_err(|_| error::Error::XTDE0420)?;
                    // Ensure namespace declarations exist for namespaced
                    // nodes appended to the result tree
                    // (XSLT 3.0 §5.7.3: namespace fixup)
                    if needs_attr_ns_fixup {
                        self.ensure_namespace_for_node(parent_node, node)?;
                    }
                    if is_element {
                        self.ensure_namespace_for_element(node)?;
                    }
                    if !is_attr_or_ns {
                        has_non_attr_child = true;
                    }
                    self.state.record_output_mutation();
                }
                sequence::Item::Atomic(atomic) => string_values.push(atomic.string_value()),
                sequence::Item::Function(function::Function::Array(array)) => {
                    for member in array.iter() {
                        self.xml_append_items(parent_node, member, string_values)?;
                    }
                    // Re-derive after recursion: children may have been added
                    if parent_is_element && !has_non_attr_child {
                        has_non_attr_child = self.state.xot.children(parent_node).any(|child| {
                            !self.state.xot.is_attribute_node(child)
                                && !self.state.xot.is_namespace_node(child)
                        });
                    }
                }
                sequence::Item::Function(_) => return Err(error::Error::XTDE0450),
            }
        }

        Ok(())
    }

    fn xml_append_string_values(&mut self, parent_node: xot::Node, string_values: &[String]) {
        let text = string_values.join(" ");
        let text_node = self.state.xot.new_text(&text);
        self.state.xot.append(parent_node, text_node).unwrap();
        self.state.record_output_mutation();
    }

    fn shallow_copy_node(&mut self, node: xot::Node) -> xot::Node {
        let xot = &mut self.state.xot;
        let value = xot.value(node);
        match value {
            // root and element are shallow copies
            xot::Value::Document => xot.new_document(),
            xot::Value::Element(element) => {
                let copy = xot.new_element(element.name());
                // Copy all in-scope namespace bindings (including inherited)
                let ns_bindings: Vec<_> = xot.namespaces_in_scope(node).collect();
                for (prefix_id, namespace_id) in ns_bindings {
                    let namespace_node = xot.new_namespace_node(prefix_id, namespace_id);
                    xot.any_append(copy, namespace_node).unwrap();
                }
                copy
            }
            // we can clone (deep-copy) these nodes as it's the same
            // operation as shallow copy
            _ => xot.clone_node(node),
        }
    }

    /// Ensure that a namespaced element has a namespace declaration for its
    /// own namespace.  Uses the empty prefix (default namespace) since
    /// computed element names from `xsl:element` are typically unprefixed.
    /// This implements XSLT 3.0 §5.7.3 namespace fixup for elements.
    ///
    /// Called when appending an element to the result tree.  At this point
    /// any compiler-generated namespace declarations (e.g. for prefixed
    /// names) have already been attached, so we only add a default-namespace
    /// binding when no declaration for this namespace exists yet.
    fn ensure_namespace_for_element(
        &mut self,
        element: xot::Node,
    ) -> error::Result<()> {
        let Some(name_id) = self.state.xot.node_name(element) else {
            return Ok(());
        };
        let namespace_id = self.state.xot.namespace_for_name(name_id);
        if namespace_id == self.state.xot.no_namespace() {
            return Ok(());
        }
        // If the element already has a namespace binding (e.g. from a
        // compiler-generated xmlns:prefix declaration), don't add another.
        if self
            .state
            .xot
            .prefix_for_namespace(element, namespace_id)
            .is_some()
        {
            return Ok(());
        }
        // Use the empty prefix (default namespace) for the element's own
        // namespace.  This produces clean output like:
        //   <math xmlns="http://www.w3.org/1998/Math/MathML">
        // rather than synthetic prefixes like n0:math.
        let empty_prefix = self.state.xot.empty_prefix();
        let ns_node = self.state.xot.new_namespace_node(empty_prefix, namespace_id);
        self.state.xot.any_append(element, ns_node)?;
        Ok(())
    }

    /// Ensure that a namespace declaration exists on `parent` for the
    /// namespace used by `node` (attribute or element).  This implements the
    /// XSLT 3.0 §5.7.3 namespace fixup rule: when an attribute whose name is
    /// in a namespace is added to an element, a namespace binding must be
    /// present.
    fn ensure_namespace_for_node(
        &mut self,
        parent: xot::Node,
        node: xot::Node,
    ) -> error::Result<()> {
        let Some(name_id) = self.state.xot.node_name(node) else {
            return Ok(());
        };
        let namespace_id = self.state.xot.namespace_for_name(name_id);
        let no_namespace = self.state.xot.no_namespace();
        if namespace_id == no_namespace {
            return Ok(());
        }
        // Check if the parent already has a prefix for this namespace
        if self
            .state
            .xot
            .prefix_for_namespace(parent, namespace_id)
            .is_some()
        {
            return Ok(());
        }
        // No prefix found — generate one.  create_missing_prefixes uses "n0",
        // "n1" etc. but we can do the same thing scoped to just this element.
        let mut i = 0u32;
        let prefix_id = loop {
            let candidate = format!("ns{i}");
            let candidate_id = self.state.xot.add_prefix(&candidate);
            // Make sure this prefix isn't already bound to a different namespace
            if self
                .state
                .xot
                .namespace_for_prefix(parent, candidate_id)
                .is_none()
            {
                break candidate_id;
            }
            i += 1;
        };
        let ns_node = self.state.xot.new_namespace_node(prefix_id, namespace_id);
        self.state.xot.any_append(parent, ns_node)?;
        Ok(())
    }

    fn apply_templates_sequence(
        &mut self,
        mode: pattern::ModeId,
        sequence: sequence::Sequence,
        params: &function::Map,
        tunnel_params: &function::Map,
        builtin_template_params_passthrough: bool,
    ) -> error::Result<sequence::Sequence> {
        let mut r: Vec<sequence::Item> = Vec::new();
        let size = sequence.len();
        let options = ApplyTemplatesOptions {
            params,
            tunnel_params,
            builtin_template_params_passthrough,
        };

        for (i, item) in sequence.iter().enumerate() {
            let sequence =
                self.apply_templates_item(mode, item.clone(), i, size, &options)?;
            if let Some(sequence) = sequence {
                for item in sequence.iter() {
                    r.push(item.clone());
                }
            }
        }
        Ok(r.into())
    }

    fn apply_templates_item(
        &mut self,
        mode: pattern::ModeId,
        item: sequence::Item,
        position: usize,
        size: usize,
        options: &ApplyTemplatesOptions<'_>,
    ) -> error::Result<Option<sequence::Sequence>> {
        let mode_declaration = self.current_program().declarations.mode(mode);
        if self.mode_requires_typed_nodes(mode_declaration) && self.item_is_untyped_node(&item) {
            return Err(error::Error::XTTE3100);
        }

        let matched_rule = self.lookup_template_rule(mode, &item);

        if let Some((_, true)) = &matched_rule {
            let on_multiple_match = mode_declaration
                .on_multiple_match
                .unwrap_or_else(|| self.runnable.dynamic_context().on_multiple_match());
            if matches!(on_multiple_match, crate::declaration::OnMultipleMatch::Fail) {
                return Err(error::Error::XTRE0540);
            }
        }

        let function_id = matched_rule.map(|(rule, _)| rule.function_id);

        if let Some(function_id) = function_id {
            let position = position + 1;
            let function = self.inline_function_value(function_id, Vec::new());
            self.mode_stack.push(mode);
            self.template_rule_stack.push(function_id);
            self.template_rule_context_stack.push(TemplateRuleContext {
                item: item.clone(),
                position,
                size,
            });
            let result = self.call_template_with_params(
                &function,
                [
                    Some(item.into()),
                    Some(atomic::Atomic::from(position as i64).into()),
                    Some(atomic::Atomic::from(size as i64).into()),
                ],
                options.params,
                options.tunnel_params,
            );
            self.template_rule_context_stack.pop();
            self.template_rule_stack.pop();
            self.mode_stack.pop();
            result.map(Some)
        } else {
            self.apply_builtin_template_rule(
                mode,
                item,
                options.params,
                options.tunnel_params,
                options.builtin_template_params_passthrough,
            )
        }
    }

    fn apply_builtin_template_rule(
        &mut self,
        mode: pattern::ModeId,
        item: sequence::Item,
        params: &function::Map,
        tunnel_params: &function::Map,
        builtin_template_params_passthrough: bool,
    ) -> error::Result<Option<sequence::Sequence>> {
        if let sequence::Item::Function(function::Function::Array(array)) = item {
            return self.apply_builtin_array_rule(
                mode,
                array,
                params,
                tunnel_params,
                builtin_template_params_passthrough,
            );
        }

        let mode_declaration = self.current_program().declarations.mode(mode);
        match mode_declaration.on_no_match {
            declaration::ModeOnNoMatch::ShallowCopy => self.apply_builtin_shallow_copy_rule(
                mode,
                item,
                params,
                tunnel_params,
                builtin_template_params_passthrough,
            ),
            declaration::ModeOnNoMatch::ShallowSkip => self.apply_builtin_shallow_skip_rule(
                mode,
                item,
                params,
                tunnel_params,
                builtin_template_params_passthrough,
            ),
            declaration::ModeOnNoMatch::DeepSkip => self.apply_builtin_deep_skip_rule(
                mode,
                item,
                params,
                tunnel_params,
                builtin_template_params_passthrough,
            ),
            declaration::ModeOnNoMatch::DeepCopy => self.apply_builtin_deep_copy_rule(item),
            declaration::ModeOnNoMatch::Fail => Err(error::Error::Unsupported(
                "xsl:mode on-no-match=\"fail\" is not supported yet".to_string(),
            )),
            declaration::ModeOnNoMatch::TextOnlyCopy => self.apply_builtin_text_only_copy_rule(
                mode,
                item,
                params,
                tunnel_params,
                builtin_template_params_passthrough,
            ),
        }
    }

    fn apply_builtin_array_rule(
        &mut self,
        mode: pattern::ModeId,
        array: function::Array,
        params: &function::Map,
        tunnel_params: &function::Map,
        builtin_template_params_passthrough: bool,
    ) -> error::Result<Option<sequence::Sequence>> {
        let members = array
            .iter()
            .flat_map(|member| member.iter())
            .collect::<Vec<_>>();
        let empty_params = function::Map::new(Vec::new()).unwrap();
        let params = if builtin_template_params_passthrough {
            params
        } else {
            &empty_params
        };

        self.apply_templates_sequence(
            mode,
            members.into(),
            params,
            tunnel_params,
            builtin_template_params_passthrough,
        )
        .map(Some)
    }

    fn apply_builtin_text_only_copy_rule(
        &mut self,
        mode: pattern::ModeId,
        item: sequence::Item,
        params: &function::Map,
        tunnel_params: &function::Map,
        builtin_template_params_passthrough: bool,
    ) -> error::Result<Option<sequence::Sequence>> {
        match item {
            sequence::Item::Node(node) => match self.state.xot.value(node) {
                xot::Value::Document | xot::Value::Element(_) => {
                    let children = self
                        .state
                        .xot
                        .children(node)
                        .map(sequence::Item::Node)
                        .collect::<Vec<_>>();
                    let empty_params = function::Map::new(Vec::new()).unwrap();
                    let params = if builtin_template_params_passthrough {
                        params
                    } else {
                        &empty_params
                    };
                    self.apply_templates_sequence(
                        mode,
                        children.into(),
                        params,
                        tunnel_params,
                        builtin_template_params_passthrough,
                    )
                    .map(Some)
                }
                xot::Value::Text(text) => {
                    let text = text.get().to_string();
                    let text_node = self.state.xot.new_text(&text);
                    Ok(Some(sequence::Item::Node(text_node).into()))
                }
                xot::Value::Attribute(attribute) => {
                    // When an attribute node is directly applied (not as part of element children),
                    // text-only-copy should output its text value
                    let value = attribute.value().to_string();
                    let text_node = self.state.xot.new_text(&value);
                    Ok(Some(sequence::Item::Node(text_node).into()))
                }
                _ => Ok(None),
            },
            sequence::Item::Atomic(atomic) => {
                let text = atomic.string_value();
                let text_node = self.state.xot.new_text(&text);
                Ok(Some(sequence::Item::Node(text_node).into()))
            }
            sequence::Item::Function(_) => Ok(None),
        }
    }

    fn apply_builtin_shallow_skip_rule(
        &mut self,
        mode: pattern::ModeId,
        item: sequence::Item,
        params: &function::Map,
        tunnel_params: &function::Map,
        builtin_template_params_passthrough: bool,
    ) -> error::Result<Option<sequence::Sequence>> {
        match item {
            sequence::Item::Node(node)
                if matches!(
                    self.state.xot.value(node),
                    xot::Value::Document | xot::Value::Element(_)
                ) =>
            {
                // For shallow-skip: process attributes and children (but not the element itself)
                let mut content: Vec<sequence::Item> = self
                    .state
                    .xot
                    .attributes(node)
                    .keys()
                    .filter_map(|name| self.state.xot.attributes(node).get_node(name))
                    .map(sequence::Item::Node)
                    .collect();
                content.extend(self.state.xot.children(node).map(sequence::Item::Node));

                let empty_params = function::Map::new(Vec::new()).unwrap();
                let params = if builtin_template_params_passthrough {
                    params
                } else {
                    &empty_params
                };
                self.apply_templates_sequence(
                    mode,
                    content.into(),
                    params,
                    tunnel_params,
                    builtin_template_params_passthrough,
                )
                .map(Some)
            }
            _ => Ok(None),
        }
    }

    fn apply_builtin_deep_skip_rule(
        &mut self,
        mode: pattern::ModeId,
        item: sequence::Item,
        params: &function::Map,
        tunnel_params: &function::Map,
        builtin_template_params_passthrough: bool,
    ) -> error::Result<Option<sequence::Sequence>> {
        match item {
            sequence::Item::Node(node)
                if matches!(self.state.xot.value(node), xot::Value::Document) =>
            {
                let children = self
                    .state
                    .xot
                    .children(node)
                    .map(sequence::Item::Node)
                    .collect::<Vec<_>>();
                let empty_params = function::Map::new(Vec::new()).unwrap();
                let params = if builtin_template_params_passthrough {
                    params
                } else {
                    &empty_params
                };
                self.apply_templates_sequence(
                    mode,
                    children.into(),
                    params,
                    tunnel_params,
                    builtin_template_params_passthrough,
                )
                .map(Some)
            }
            _ => Ok(None),
        }
    }

    fn apply_builtin_shallow_copy_rule(
        &mut self,
        mode: pattern::ModeId,
        item: sequence::Item,
        params: &function::Map,
        tunnel_params: &function::Map,
        builtin_template_params_passthrough: bool,
    ) -> error::Result<Option<sequence::Sequence>> {
        match item {
            sequence::Item::Node(node) => match self.state.xot.value(node) {
                xot::Value::Document => {
                    let copy = self.state.xot.new_document();
                    let children = self
                        .state
                        .xot
                        .children(node)
                        .map(sequence::Item::Node)
                        .collect::<Vec<_>>();
                    let content = self.apply_templates_sequence(
                        mode,
                        children.into(),
                        params,
                        tunnel_params,
                        builtin_template_params_passthrough,
                    )?;
                    self.xml_append(copy, content)?;
                    Ok(Some(sequence::Item::Node(copy).into()))
                }
                xot::Value::Element(element) => {
                    let copy = self.state.xot.new_element(element.name());

                    let namespace_nodes = self
                        .state
                        .xot
                        .namespaces(node)
                        .keys()
                        .filter_map(|prefix| self.state.xot.namespaces(node).get_node(prefix))
                        .collect::<Vec<_>>();
                    for namespace_node in namespace_nodes {
                        let namespace_copy = self.state.xot.clone_node(namespace_node);
                        self.state.xot.any_append(copy, namespace_copy).unwrap();
                    }

                    let mut content = self
                        .state
                        .xot
                        .attributes(node)
                        .keys()
                        .filter_map(|name| self.state.xot.attributes(node).get_node(name))
                        .map(sequence::Item::Node)
                        .collect::<Vec<_>>();
                    content.extend(self.state.xot.children(node).map(sequence::Item::Node));

                    let content = self.apply_templates_sequence(
                        mode,
                        content.into(),
                        params,
                        tunnel_params,
                        builtin_template_params_passthrough,
                    )?;
                    self.xml_append(copy, content)?;
                    Ok(Some(sequence::Item::Node(copy).into()))
                }
                _ => Ok(Some(
                    sequence::Item::Node(self.state.xot.clone_node(node)).into(),
                )),
            },
            sequence::Item::Atomic(_) | sequence::Item::Function(_) => Ok(None),
        }
    }

    fn apply_builtin_deep_copy_rule(
        &mut self,
        item: sequence::Item,
    ) -> error::Result<Option<sequence::Sequence>> {
        match item {
            sequence::Item::Node(node) => Ok(Some(
                sequence::Item::Node(self.state.xot.clone_node(node)).into(),
            )),
            sequence::Item::Atomic(_) | sequence::Item::Function(_) => Ok(None),
        }
    }

    fn mode_requires_typed_nodes(&self, mode_declaration: declaration::ModeDeclaration) -> bool {
        matches!(mode_declaration.typed, declaration::ModeTyped::Yes)
    }

    fn item_is_untyped_node(&self, item: &sequence::Item) -> bool {
        match item {
            sequence::Item::Node(node) => matches!(
                self.state.xot.value(*node),
                xot::Value::Document | xot::Value::Element(_) | xot::Value::Attribute(_)
            ),
            sequence::Item::Atomic(_) | sequence::Item::Function(_) => false,
        }
    }

    pub(crate) fn call_template_with_params(
        &mut self,
        function: &function::Function,
        context_arguments: [Option<sequence::Sequence>; 3],
        params: &function::Map,
        tunnel_params: &function::Map,
    ) -> error::Result<sequence::Sequence> {
        let (function_id, program) = match function {
            function::Function::Inline(data) => (
                data.id,
                data.program.as_deref().unwrap_or_else(|| self.runnable.program()),
            ),
            _ => return Err(error::Error::type_error("expected a template function")),
        };

        // Fast path: if no new tunnel params are being added, avoid cloning
        // the current tunnel params map entirely.
        let effective_tunnel_params = if tunnel_params.is_empty() {
            self.current_tunnel_params().clone()
        } else {
            let mut effective = self.current_tunnel_params().clone();
            for (key, value) in tunnel_params.entries() {
                effective = effective.put(key.clone(), value)?;
            }
            effective
        };

        let mut arguments = context_arguments.into_iter().collect::<Vec<_>>();
        let parameter_types = function.signature(self.runnable.program()).parameter_types().to_vec();
        if let Some(template_params) = program.declarations.template_params(function_id) {
            for (index, param) in template_params.iter().enumerate() {
                let key = atomic::Atomic::from(param.name.as_str());
                let value = if param.tunnel {
                    effective_tunnel_params.get(&key).cloned()
                } else {
                    params.get(&key).cloned()
                };
                let parameter_type = parameter_types.get(index + 3).cloned().unwrap_or(None);
                let value = self.coerce_template_argument(
                    value,
                    parameter_type.as_ref(),
                    &param.name,
                )?;
                arguments.push(value);
            }
        }

        let focus_absent = arguments.iter().take(3).all(|a| a.is_none());
        self.focus_absent_stack.push(focus_absent);
        self.tunnel_params.push(effective_tunnel_params);
        let result = self.call_function_with_optional_arguments(function, arguments);
        self.tunnel_params.pop();
        self.focus_absent_stack.pop();
        result
    }

    fn current_tunnel_params(&self) -> &function::Map {
        self.tunnel_params.last().unwrap()
    }

    fn pop_optional_sequence(&mut self) -> Option<sequence::Sequence> {
        match self.state.pop_value() {
            stack::Value::Absent => None,
            stack::Value::Sequence(sequence) => Some(sequence),
        }
    }

    fn coerce_template_argument(
        &mut self,
        value: Option<sequence::Sequence>,
        parameter_type: Option<&ast::SequenceType>,
        param_name: &str,
    ) -> error::Result<Option<sequence::Sequence>> {
        let Some(value) = value else {
            return Ok(None);
        };
        let Some(parameter_type) = parameter_type else {
            return Ok(Some(value));
        };

        value
            .sequence_type_matching_function_conversion(
                parameter_type,
                self.runnable.static_context(),
                self.state.xot(),
                &|function| function.signature(self.runnable.program()).clone(),
            )
            .map(Some)
            .map_err(|e| {
                let expected = parameter_type.display_representation();
                let detail = match e {
                    error::Error::XPTY0004(Some(ref context)) => {
                        format!(
                            "parameter ${}: expected {}, {}",
                            param_name, expected, context
                        )
                    }
                    _ => {
                        format!(
                            "parameter ${}: cannot convert to {}",
                            param_name, expected
                        )
                    }
                };
                error::Error::XTTE0590(Some(detail))
            })
    }

    pub(crate) fn lookup_pattern(
        &mut self,
        mode: pattern::ModeId,
        item: &sequence::Item,
    ) -> Option<function::InlineFunctionId> {
        self.lookup_template_rule(mode, item)
            .map(|(rule, _)| rule.function_id)
    }

    fn lookup_template_rule(
        &mut self,
        mode: pattern::ModeId,
        item: &sequence::Item,
    ) -> Option<(crate::declaration::TemplateRule, bool)> {
        // Pre-compute name_id and ensure the name index is built while we can
        // safely borrow xot immutably — before the `matches` closure captures
        // &mut self.
        let name_id = pattern::pattern_lookup::item_name_id(item, self.xot());
        self.runnable
            .program()
            .declarations
            .mode_lookup
            .ensure_index(mode, self.xot());
        self.name_cache = self
            .runnable
            .program()
            .declarations
            .mode_lookup
            .name_cache_rc(mode);
        let result = self
            .runnable
            .program()
            .declarations
            .mode_lookup
            .lookup_with_ambiguity(
                mode,
                name_id,
                |pattern| self.matches(pattern, item),
                |a, b| {
                    a.function_id != b.function_id
                        && a.import_precedence == b.import_precedence
                        && a.priority == b.priority
                },
            )
            .map(|(rule, ambiguous)| (rule.clone(), ambiguous));
        self.name_cache = None;
        result
    }

    fn lookup_pattern_after(
        &mut self,
        mode: pattern::ModeId,
        current: function::InlineFunctionId,
        item: &sequence::Item,
    ) -> Option<function::InlineFunctionId> {
        let name_id = pattern::pattern_lookup::item_name_id(item, self.xot());
        self.runnable
            .program()
            .declarations
            .mode_lookup
            .ensure_index(mode, self.xot());
        self.name_cache = self
            .runnable
            .program()
            .declarations
            .mode_lookup
            .name_cache_rc(mode);
        let result = self
            .runnable
            .program()
            .declarations
            .mode_lookup
            .lookup_after(
                mode,
                name_id,
                |pattern| self.matches(pattern, item),
                |rule| rule.function_id == current,
            )
            .map(|rule| rule.function_id);
        self.name_cache = None;
        result
    }

    fn lookup_pattern_after_lower_import_precedence(
        &mut self,
        mode: pattern::ModeId,
        current: function::InlineFunctionId,
        current_import_precedence: i64,
        current_module_path: &[usize],
        item: &sequence::Item,
    ) -> Option<function::InlineFunctionId> {
        let import_precedences = self
            .runnable
            .program()
            .declarations
            .template_import_precedences()
            .clone();
        let module_paths = self
            .runnable
            .program()
            .declarations
            .template_module_paths()
            .clone();
        let name_id = pattern::pattern_lookup::item_name_id(item, self.xot());
        self.runnable
            .program()
            .declarations
            .mode_lookup
            .ensure_index(mode, self.xot());
        self.name_cache = self
            .runnable
            .program()
            .declarations
            .mode_lookup
            .name_cache_rc(mode);
        let result = self
            .runnable
            .program()
            .declarations
            .mode_lookup
            .lookup_after_lower_import_precedence(
                mode,
                name_id,
                current_import_precedence,
                |pattern| self.matches(pattern, item),
                |rule| rule.function_id == current,
                |rule| {
                    import_precedences
                        .get(&rule.function_id)
                        .copied()
                        .unwrap_or_default()
                },
                |rule| {
                    module_paths
                        .get(&rule.function_id)
                        .is_some_and(|module_path| {
                            module_path.len() > current_module_path.len()
                                && module_path.starts_with(current_module_path)
                        })
                },
            )
            .map(|rule| rule.function_id);
        self.name_cache = None;
        result
    }

    fn current_mode_or_fallback(&self, fallback: pattern::ModeId) -> pattern::ModeId {
        self.mode_stack.last().copied().unwrap_or(fallback)
    }

    fn continue_template_with_params(
        &mut self,
        params: &function::Map,
        tunnel_params: &function::Map,
        behavior: u8,
    ) -> error::Result<sequence::Sequence> {
        let mode = *self.mode_stack.last().ok_or(error::Error::XTDE0560)?;
        let current_function = *self
            .template_rule_stack
            .last()
            .ok_or(error::Error::XTDE0560)?;
        // If the current template has context-item use="absent", the focus is absent
        // and xsl:next-match / xsl:apply-imports cannot be used (XTDE0560).
        if self.focus_absent_stack.last().copied().unwrap_or(false) {
            return Err(error::Error::XTDE0560);
        }
        let context = self
            .template_rule_context_stack
            .last()
            .ok_or(error::Error::XTDE0560)?
            .clone();
        let item = context.item;
        let position = context.position;
        let size = context.size;
        let next_function = match behavior {
            0 => self.lookup_pattern_after(mode, current_function, &item),
            1 => {
                let current_import_precedence = self
                    .runnable
                    .program()
                    .declarations
                    .template_import_precedence(current_function)
                    .unwrap_or_default();
                let current_module_path = self
                    .runnable
                    .program()
                    .declarations
                    .template_module_path(current_function)
                    .map(ToOwned::to_owned)
                    .unwrap_or_default();
                self.lookup_pattern_after_lower_import_precedence(
                    mode,
                    current_function,
                    current_import_precedence,
                    &current_module_path,
                    &item,
                )
            }
            _ => {
                return Err(error::Error::Unsupported(format!(
                    "Unknown continue-template behavior: {behavior}"
                )))
            }
        };

        if let Some(function_id) = next_function {
            let function = self.inline_function_value(function_id, Vec::new());
            self.mode_stack.push(mode);
            self.template_rule_stack.push(function_id);
            self.template_rule_context_stack.push(TemplateRuleContext {
                item: item.clone(),
                position,
                size,
            });
            let result = self.call_template_with_params(
                &function,
                [
                    Some(item.clone().into()),
                    Some(atomic::Atomic::from(position as i64).into()),
                    Some(atomic::Atomic::from(size as i64).into()),
                ],
                params,
                tunnel_params,
            );
            self.template_rule_context_stack.pop();
            self.template_rule_stack.pop();
            self.mode_stack.pop();
            result
        } else {
            self.apply_builtin_template_rule(mode, item, params, tunnel_params, true)
                .map(|value| value.unwrap_or_default())
        }
    }

    // The interpreter can return an error for any byte code, in any level of
    // nesting in the function. When this happens the interpreter stops with
    // the error code. We here wrap it in a SpannedError using the current
    // span.
    pub(crate) fn err(&self, value_error: error::Error) -> error::SpannedError {
        let detail = value_error.detail().map(|s| s.to_string());
        error::SpannedError {
            error: value_error,
            span: Some(self.current_span()),
            detail,
            contexts: self.error_contexts.clone(),
        }
    }

    /// Push an error context breadcrumb onto the stack.
    fn push_error_context(&mut self, label: impl Into<String>) {
        self.error_contexts.push(error::ErrorContext {
            span: self.current_span(),
            label: label.into(),
        });
    }

    /// Pop the most recent error context breadcrumb.
    fn pop_error_context(&mut self) {
        self.error_contexts.pop();
    }

    // During the compilation process, spans became associated with each
    // compiled bytecode instruction. Here we take the current function and the
    // instruction in it to determine the span of the code that failed.
    fn current_span(&self) -> SourceSpan {
        let frame = self.state.frame();
        let function = self.current_program().inline_function(frame.function());
        // we substract 1 to end up in the current instruction - this
        // because the ip is already on the next instruction
        function.spans[frame.ip - 1]
    }

    fn read_instruction(&mut self) -> EncodedInstruction {
        let function_id = self.state.frame().function();
        let mut ip = self.state.frame().ip;
        let function = self.current_program().inline_function(function_id);
        let chunk = &function.chunk;
        let instruction = read_instruction(chunk, &mut ip);
        self.state.frame_mut().ip = ip;
        instruction
    }

    fn read_u16(&mut self) -> u16 {
        let function_id = self.state.frame().function();
        let mut ip = self.state.frame().ip;
        let function = self.current_program().inline_function(function_id);
        let chunk = &function.chunk;
        let value = read_u16(chunk, &mut ip);
        self.state.frame_mut().ip = ip;
        value
    }

    fn read_i16(&mut self) -> i16 {
        let function_id = self.state.frame().function();
        let mut ip = self.state.frame().ip;
        let function = self.current_program().inline_function(function_id);
        let chunk = &function.chunk;
        let value = read_i16(chunk, &mut ip);
        self.state.frame_mut().ip = ip;
        value
    }

    fn read_u8(&mut self) -> u8 {
        let function_id = self.state.frame().function();
        let mut ip = self.state.frame().ip;
        let function = self.current_program().inline_function(function_id);
        let chunk = &function.chunk;
        let value = read_u8(chunk, &mut ip);
        self.state.frame_mut().ip = ip;
        value
    }
}
