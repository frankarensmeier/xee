use xee_xpath::error::ErrorValue;

use super::assert::Failure;

#[derive(Debug, PartialEq)]
pub struct UnexpectedError(pub String);

#[derive(Debug, PartialEq)]
pub enum TestOutcome {
    Passed,
    UnexpectedError(UnexpectedError),
    Failed(Failure),
    RuntimeError(ErrorValue),
    CompilationError(ErrorValue),
    UnsupportedExpression(ErrorValue),
    Unsupported,
    EnvironmentError(String),
    Panic,
}

impl TestOutcome {
    pub(crate) fn is_exactly_passed(&self) -> bool {
        matches!(self, Self::Passed)
    }

    pub(crate) fn category(&self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed(_) => "FAILED",
            Self::UnexpectedError(_) => "WRONG ERROR",
            Self::RuntimeError(_) => "RUNTIME ERROR",
            Self::CompilationError(_) => "COMPILATION ERROR",
            Self::UnsupportedExpression(_) => "UNSUPPORTED EXPR",
            Self::Unsupported => "UNSUPPORTED",
            Self::EnvironmentError(_) => "ENV ERROR",
            Self::Panic => "PANIC",
        }
    }
}
