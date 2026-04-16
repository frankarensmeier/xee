use std::rc::Rc;

use crate::{context, stack};
use crate::interpreter::Program;
use xee_name::Name;
use xee_name::FN_NAMESPACE;
use xee_schema_type::Xs;
use xee_xpath_ast::{ast, Pattern};
use xot::xmlname::NameStrInfo;

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
    Coerced(Rc<CoercedFunctionData>),
    Concat(Rc<ConcatFunctionData>),
    PatternMatcher(Rc<PatternMatcherFunctionData>),
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

#[derive(Debug, PartialEq)]
pub struct CoercedFunctionData {
    pub(crate) function: Box<Function>,
    pub(crate) signature: super::Signature,
}

#[derive(Debug, PartialEq)]
pub struct ConcatFunctionData {
    pub(crate) arity: usize,
    pub(crate) signature: super::Signature,
}

#[derive(Debug, PartialEq)]
pub struct PatternMatcherFunctionData {
    pub(crate) pattern: Pattern<InlineFunctionId>,
    pub(crate) signature: super::Signature,
}

impl From<CoercedFunctionData> for Function {
    fn from(data: CoercedFunctionData) -> Self {
        Self::Coerced(Rc::new(data))
    }
}

impl CoercedFunctionData {
    pub(crate) fn new(function: Function, signature: super::Signature) -> Self {
        CoercedFunctionData {
            function: Box::new(function),
            signature,
        }
    }
}

impl From<ConcatFunctionData> for Function {
    fn from(data: ConcatFunctionData) -> Self {
        Self::Concat(Rc::new(data))
    }
}

impl ConcatFunctionData {
    pub fn new(arity: usize) -> Self {
        let arg_type = ast::SequenceType::Item(ast::Item {
            occurrence: ast::Occurrence::Option,
            item_type: ast::ItemType::AtomicOrUnionType(Xs::AnyAtomicType),
        });
        let return_type = ast::SequenceType::Item(ast::Item {
            occurrence: ast::Occurrence::One,
            item_type: ast::ItemType::AtomicOrUnionType(Xs::String),
        });
        Self {
            arity,
            signature: super::Signature::new(vec![Some(arg_type); arity], Some(return_type)),
        }
    }

    fn name(&self) -> Name {
        Name::new(
            "concat".to_string(),
            FN_NAMESPACE.to_string(),
            String::new(),
        )
    }
}

impl From<PatternMatcherFunctionData> for Function {
    fn from(data: PatternMatcherFunctionData) -> Self {
        Self::PatternMatcher(Rc::new(data))
    }
}

impl PatternMatcherFunctionData {
    pub fn new(pattern: Pattern<InlineFunctionId>) -> Self {
        let item_type = ast::SequenceType::Item(ast::Item {
            occurrence: ast::Occurrence::One,
            item_type: ast::ItemType::KindTest(ast::KindTest::Any),
        });
        let boolean_type = ast::SequenceType::Item(ast::Item {
            occurrence: ast::Occurrence::One,
            item_type: ast::ItemType::AtomicOrUnionType(Xs::Boolean),
        });
        Self {
            pattern,
            signature: super::Signature::new(vec![Some(item_type)], Some(boolean_type)),
        }
    }
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
            Self::Coerced(_) => unreachable!(),
            Self::Concat(_) => unreachable!(),
            Self::PatternMatcher(_) => unreachable!(),
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
            Self::Inline(data) => self
                .resolved_program(default_program)
                .inline_function(data.id)
                .declared_name
                .clone(),
            Self::Coerced(data) => data.function.name(default_program),
            Self::Concat(data) => Some(data.name()),
            Self::PatternMatcher(_) => None,
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
            Self::Coerced(data) => data.signature.arity(),
            Self::Concat(data) => data.arity,
            Self::PatternMatcher(_) => 1,
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
            Self::Coerced(data) => &data.signature,
            Self::Concat(data) => &data.signature,
            Self::PatternMatcher(data) => &data.signature,
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
            Self::Coerced(data) => data.function.display_representation(xot, context),
            Self::Concat(data) => format!("{}{}", data.name().full_name(), data.signature.display_representation()),
            Self::PatternMatcher(data) => format!("function{}", data.signature.display_representation()),
            Self::Map(map) => map.display_representation(xot, context),
            Self::Array(array) => array.display_representation(xot, context),
        }
    }
}
