/// XSLT has a number of things that can be declared globally, such
/// as global variables, parameters, functions, and templates. This
/// contains the runtime information to execute XSLT.
mod decl;
mod globalvar;

pub use decl::{
    AccumulatorDeclaration, AccumulatorPhase, AccumulatorRuleDeclaration, Declarations,
    GlobalVariableDeclaration, KeyDeclaration, ModeDeclaration, ModeOnNoMatch, ModeTyped,
    NamedTemplateDeclaration, NumberPatternDeclaration, OnMultipleMatch,
    TemplateParamDeclaration, TemplateRule,
};
