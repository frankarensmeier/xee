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

    pub(crate) fn detail(&self) -> String {
        match self {
            Self::Passed => String::new(),
            Self::Failed(failure) => format!("{}", failure),
            Self::UnexpectedError(UnexpectedError(code)) => format!("code: {}", code),
            Self::RuntimeError(error) => format!("{}", error),
            Self::CompilationError(error) => format!("{}", error),
            Self::UnsupportedExpression(error) => format!("{}", error),
            Self::Unsupported => String::new(),
            Self::EnvironmentError(error) => error.clone(),
            Self::Panic => String::new(),
        }
    }
}
