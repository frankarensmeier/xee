use xot::{Node, Xot};

use std::path::{Path, PathBuf};

use xee_interpreter::context::{StaticContextBuilder, Variables};
use xee_interpreter::error;
use xee_interpreter::interpreter::Program;
use xee_interpreter::sequence;

use crate::ast_ir::{parse, parse_with_base_dir};

pub fn evaluate_program(
    xot: &mut Xot,
    program: &Program,
    root: Node,
) -> error::SpannedResult<sequence::Sequence> {
    evaluate_program_with_variables(xot, program, root, Variables::new())
}

pub fn evaluate_program_with_variables(
    xot: &mut Xot,
    program: &Program,
    root: Node,
    variables: Variables,
) -> error::SpannedResult<sequence::Sequence> {
    let mut documents = xee_interpreter::xml::Documents::new();
    let handle = documents.add_root(None, root).unwrap();
    let root = documents.get_node_by_handle(handle).unwrap();
    let mut dynamic_context_builder = program.dynamic_context_builder();
    dynamic_context_builder.context_node(root);
    dynamic_context_builder.documents(documents);
    dynamic_context_builder.variables(variables);
    let context = dynamic_context_builder.build();
    let runnable = program.runnable(&context);
    runnable.many(xot)
}

pub fn evaluate(xot: &mut Xot, xml: &str, xslt: &str) -> error::SpannedResult<sequence::Sequence> {
    let base_dir = std::env::current_dir().ok();
    evaluate_with_base_dir(xot, xml, xslt, base_dir, None)
}

pub fn evaluate_with_stylesheet_path(
    xot: &mut Xot,
    xml: &str,
    xslt: &str,
    stylesheet_path: &Path,
) -> error::SpannedResult<sequence::Sequence> {
    let program = parse_with_stylesheet_path(xslt, stylesheet_path)?;
    let root = xot.parse(xml).unwrap();
    evaluate_program(xot, &program, root)
}

pub fn parse_with_stylesheet_path(
    xslt: &str,
    stylesheet_path: &Path,
) -> error::SpannedResult<Program> {
    let canonical = stylesheet_path
        .canonicalize()
        .unwrap_or_else(|_| stylesheet_path.to_path_buf());
    let base_dir = canonical
        .parent()
        .or_else(|| stylesheet_path.parent())
        .map(Path::to_path_buf);
    let static_base_uri = format!("file://{}", canonical.display())
        .replace(' ', "%20")
        .try_into()
        .ok();

    let mut static_context_builder = StaticContextBuilder::default();
    static_context_builder.static_base_uri(static_base_uri);
    let static_context = static_context_builder.build();
    match base_dir {
        Some(base_dir) => parse_with_base_dir(static_context, xslt, Some(base_dir)),
        None => parse(static_context, xslt),
    }
}

pub fn parse_to_ir_with_stylesheet_path(
    xslt: &str,
    stylesheet_path: &Path,
) -> error::SpannedResult<xee_ir::ir::Declarations> {
    let canonical = stylesheet_path
        .canonicalize()
        .unwrap_or_else(|_| stylesheet_path.to_path_buf());
    let base_dir = canonical
        .parent()
        .or_else(|| stylesheet_path.parent())
        .map(Path::to_path_buf);
    let static_base_uri = format!("file://{}", canonical.display())
        .replace(' ', "%20")
        .try_into()
        .ok();

    let mut static_context_builder = StaticContextBuilder::default();
    static_context_builder.static_base_uri(static_base_uri);
    let static_context = static_context_builder.build();
    crate::parse_to_ir(static_context, xslt, base_dir, None)
}

pub fn evaluate_with_base_dir(
    xot: &mut Xot,
    xml: &str,
    xslt: &str,
    base_dir: Option<PathBuf>,
    static_base_uri: Option<iri_string::types::IriAbsoluteString>,
) -> error::SpannedResult<sequence::Sequence> {
    let mut static_context_builder = StaticContextBuilder::default();
    static_context_builder.static_base_uri(static_base_uri);
    let static_context = static_context_builder.build();
    let root = xot.parse(xml).unwrap();
    let program = match base_dir {
        Some(base_dir) => parse_with_base_dir(static_context, xslt, Some(base_dir))?,
        None => parse(static_context, xslt)?,
    };
    evaluate_program(xot, &program, root)
}
