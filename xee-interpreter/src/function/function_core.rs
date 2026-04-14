use std::rc::Rc;

use crate::{context, stack};
use crate::interpreter::Program;
use xee_name::Name;

use super::array::Array;
use super::map::Map;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct InlineFunctionId(pub(crate) usize);

impl InlineFunctionId {
    pub fn new(id: usize) -> Self {
        InlineFunctionId(id)
    }

    pub fn get(&self) -> usize {
        self.0
    }

    pub fn as_u16(&self) -> u16 {
        self.0 as u16
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct StaticFunctionId(pub(crate) usize);

impl StaticFunctionId {
    pub fn as_u16(&self) -> u16 {
        self.0 as u16
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Function {
    Static(Rc<StaticFunctionData>),
    Inline(Rc<InlineFunctionData>),
    Map(Map),
    Array(Array),
}

#[cfg(target_arch = "x86_64")]
static_assertions::assert_eq_size!(Function, [u8; 16]);

#[derive(Debug, PartialEq)]
pub struct StaticFunctionData {
    pub(crate) id: StaticFunctionId,
    pub(crate) closure_vars: Box<[stack::Value]>,
}

impl From<StaticFunctionData> for Function {
    fn from(data: StaticFunctionData) -> Self {
        Self::Static(Rc::new(data))
    }
}

impl StaticFunctionData {
    pub(crate) fn new(id: StaticFunctionId, closure_vars: Vec<stack::Value>) -> Self {
        StaticFunctionData {
            id,
            closure_vars: closure_vars.into(),
        }
    }
}

#[derive(Debug)]
pub struct InlineFunctionData {
    pub(crate) id: InlineFunctionId,
    pub(crate) closure_vars: Box<[stack::Value]>,
    pub(crate) program: Option<Rc<Program>>,
}

impl From<InlineFunctionData> for Function {
    fn from(data: InlineFunctionData) -> Self {
        Self::Inline(Rc::new(data))
    }
}

impl InlineFunctionData {
    pub(crate) fn new(id: InlineFunctionId, closure_vars: Vec<stack::Value>) -> Self {
        InlineFunctionData {
            id,
            closure_vars: closure_vars.into(),
            program: None,
        }
    }

    pub(crate) fn new_with_program(
        program: Rc<Program>,
        id: InlineFunctionId,
        closure_vars: Vec<stack::Value>,
    ) -> Self {
        InlineFunctionData {
            id,
            closure_vars: closure_vars.into(),
            program: Some(program),
        }
    }
}

impl PartialEq for InlineFunctionData {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.closure_vars == other.closure_vars
            && match (&self.program, &other.program) {
                (Some(left), Some(right)) => Rc::ptr_eq(left, right),
                (None, None) => true,
                _ => false,
            }
    }
}

impl Function {
    fn resolved_program<'a, 'b>(&'a self, default_program: &'b Program) -> &'a Program
    where
        'b: 'a,
    {
        match self {
            Self::Inline(data) => data.program.as_deref().unwrap_or(default_program),
            _ => default_program,
        }
    }

    pub(crate) fn closure_vars(&self) -> &[stack::Value] {
        match self {
            Self::Static(data) => &data.closure_vars,
            Self::Inline(data) => &data.closure_vars,
            _ => unreachable!(),
        }
    }

    pub(crate) fn name(&self, default_program: &Program) -> Option<Name> {
        match self {
            Self::Static(data) => self
                .resolved_program(default_program)
                .static_function(data.id)
                .name()
                .cloned(),
            _ => None,
        }
    }

    pub(crate) fn arity(&self, default_program: &Program) -> usize {
        match self {
            Self::Static(data) => self
                .resolved_program(default_program)
                .static_function(data.id)
                .arity(),
            Self::Inline(data) => self
                .resolved_program(default_program)
                .inline_function(data.id)
                .arity(),
            Self::Map(_) => 1,
            Self::Array(_) => 1,
        }
    }

    pub(crate) fn signature<'a, 'b>(
        &'a self,
        default_program: &'b Program,
    ) -> &'a super::Signature
    where
        'b: 'a,
    {
        match self {
            Self::Static(data) => self
                .resolved_program(default_program)
                .static_function(data.id)
                .signature(),
            Self::Inline(data) => self
                .resolved_program(default_program)
                .inline_function(data.id)
                .signature(),
            Self::Map(_) => default_program.map_signature(),
            Self::Array(_) => default_program.array_signature(),
        }
    }

    pub fn display_representation(
        &self,
        xot: &xot::Xot,
        context: &context::DynamicContext,
    ) -> String {
        match self {
            Self::Static(data) => {
                let function = context.static_function_by_id(data.id);
                function.display_representation()
            }
            Self::Inline(data) => {
                let function = data
                    .program
                    .as_deref()
                    .map(|program| program.inline_function(data.id))
                    .unwrap_or_else(|| context.inline_function_by_id(data.id));
                function.display_representation()
            }
            Self::Map(map) => map.display_representation(xot, context),
            Self::Array(array) => array.display_representation(xot, context),
        }
    }
}
