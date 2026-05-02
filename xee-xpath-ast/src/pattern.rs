use crate::ast;

pub use crate::ast::{NameTest, NodeTest};

// todo, put all pattern related stuff in a single package module
pub use crate::pattern_transform::transform_pattern;

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Pattern<E> {
    Predicate(PredicatePattern<E>),
    Expr(ExprPattern<E>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PredicatePattern<E> {
    pub predicates: Vec<E>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ExprPattern<E> {
    Path(PathExpr<E>),
    BinaryExpr(BinaryExpr<E>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BinaryExpr<E> {
    pub operator: Operator,
    pub left: Box<ExprPattern<E>>,
    pub right: Box<ExprPattern<E>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Operator {
    Union,
    Intersect,
    Except,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PathExpr<E> {
    pub root: PathRoot<E>,
    pub steps: Vec<StepExpr<E>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PathRoot<E> {
    Rooted { root: RootExpr, predicates: Vec<E> },
    AbsoluteSlash,
    AbsoluteDoubleSlash,
    Relative,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RootExpr {
    VarRef(ast::Name),
    FunctionCall(FunctionCall),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FunctionCall {
    pub name: OuterFunctionName,
    // one or more always
    pub args: Vec<Argument>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum OuterFunctionName {
    Doc,
    Id,
    ElementWithId,
    Key,
    Root,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Argument {
    VarRef(ast::Name),
    Literal(ast::Literal),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum StepExpr<E> {
    PostfixExpr(PostfixExpr<E>),
    AxisStep(AxisStep<E>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PostfixExpr<E> {
    pub expr: ExprPattern<E>,
    pub predicates: Vec<E>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AxisStep<E> {
    pub forward: ForwardAxis,
    pub node_test: ast::NodeTest,
    pub predicates: Vec<E>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ForwardAxisNodeTest {
    pub axis: ForwardAxis,
    pub node_test: ast::NodeTest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ForwardAxis {
    Child,
    Descendant,
    Attribute,
    Self_,
    DescendantOrSelf,
    Namespace,
}
