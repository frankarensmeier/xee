//! Precompiled stylesheet support.
//!
//! This module provides the ability to serialize a compiled XSLT stylesheet's
//! intermediate representation (IR) to disk and load it back, skipping the
//! expensive preprocessing step (XML parsing, import resolution, AST building)
//! which typically accounts for ~94% of compilation time.
//!
//! # Usage
//!
//! ## Compiling a stylesheet to a precompiled file
//!
//! ```no_run
//! use xee_xslt_compiler::precompiled;
//!
//! precompiled::compile_to_file("stylesheet.xsl", "stylesheet.xeec")
//!     .expect("compilation failed");
//! ```
//!
//! ## Loading a precompiled stylesheet
//!
//! ```no_run
//! use xee_xslt_compiler::precompiled;
//!
//! let program = precompiled::load_from_file("stylesheet.xeec")
//!     .expect("loading failed");
//! ```

use std::path::Path;

use iri_string::types::IriAbsoluteString;
use serde::{Deserialize, Serialize};
use xee_interpreter::context::{DecimalFormatSymbols, StaticContext};
use xee_interpreter::error;
use xee_ir::{compile_xslt, ir};
use xee_name::Namespaces;
use xot::xmlname::OwnedName;

use crate::dynamic_xpath::XsltDynamicXPathEvaluator;

/// Metadata required to reconstruct a `StaticContext` for IR→bytecode
/// compilation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrecompiledMetadata {
    pub namespaces: Namespaces,
    pub static_base_uri: Option<IriAbsoluteString>,
    pub default_decimal_format: DecimalFormatSymbols,
    pub decimal_formats: ahash::HashMap<OwnedName, DecimalFormatSymbols>,
    pub disabled_functions: Vec<OwnedName>,
    pub stylesheet_xslt_version: Option<u8>,
    pub processor_xslt_version: Option<u8>,
    pub processor_xpath_version: Option<u8>,
    /// Serialized form of the initial mode.
    /// None = unnamed mode, Some(name) = named mode in Clark notation.
    pub initial_mode: Option<String>,
}

/// A precompiled stylesheet bundle containing IR declarations and metadata.
#[derive(Debug, Serialize, Deserialize)]
pub struct PrecompiledStylesheet {
    /// Format version for forward compatibility.
    pub version: u32,
    pub metadata: PrecompiledMetadata,
    pub declarations: ir::Declarations,
}

/// Current format version.
const FORMAT_VERSION: u32 = 1;

impl PrecompiledMetadata {
    /// Reconstruct a `StaticContext` from the serialized metadata.
    pub fn to_static_context(&self) -> StaticContext {
        let mut ctx = StaticContext::from_namespaces(self.namespaces.clone());
        ctx.set_static_base_uri(self.static_base_uri.clone());
        ctx.set_decimal_formats(
            self.default_decimal_format.clone(),
            self.decimal_formats.clone(),
        );
        for name in &self.disabled_functions {
            ctx.disable_function(name.clone());
        }
        ctx.set_stylesheet_xslt_version(self.stylesheet_xslt_version);
        ctx.set_processor_xslt_version(self.processor_xslt_version);
        ctx.set_processor_xpath_version(self.processor_xpath_version);
        ctx
    }
}

/// Compile an XSLT stylesheet and serialize the IR to a file.
///
/// The stylesheet at `stylesheet_path` is fully preprocessed (XML parsing,
/// import resolution, AST building, IR conversion), and the resulting IR +
/// metadata are serialized to `output_path` using bincode.
pub fn compile_to_file(
    stylesheet_path: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
) -> error::SpannedResult<()> {
    let precompiled = compile_stylesheet(stylesheet_path)?;
    let data = rmp_serde::to_vec(&precompiled)
        .map_err(|e| error::SpannedError::from(error::Error::Unsupported(format!("serialization failed: {e}"))))?;
    std::fs::write(output_path, data)
        .map_err(|e| error::SpannedError::from(error::Error::Unsupported(format!("write failed: {e}"))))?;
    Ok(())
}

/// Load a precompiled stylesheet from a file and compile it to a `Program`.
///
/// This skips the expensive preprocessing step (~850ms for DocBook) and only
/// runs the fast IR→bytecode compilation (~17ms).
///
/// Deserialization runs on a thread with a larger stack (16 MiB) because the
/// IR for large stylesheets (e.g. DocBook) contains deeply nested `Expr`
/// trees that overflow the default thread stack during recursive
/// `rmp_serde::from_slice`.
pub fn load_from_file(
    precompiled_path: impl AsRef<Path>,
) -> error::SpannedResult<xee_interpreter::interpreter::Program> {
    let data = std::fs::read(precompiled_path.as_ref())
        .map_err(|e| error::SpannedError::from(error::Error::Unsupported(format!("read failed: {e}"))))?;

    // Deserialize on a thread with a 16 MiB stack to handle deeply nested IR.
    const DESER_STACK_SIZE: usize = 16 * 1024 * 1024;
    let precompiled: PrecompiledStylesheet = std::thread::Builder::new()
        .stack_size(DESER_STACK_SIZE)
        .spawn(move || -> Result<PrecompiledStylesheet, String> {
            rmp_serde::from_slice(&data).map_err(|e| format!("deserialization failed: {e}"))
        })
        .map_err(|e| {
            error::SpannedError::from(error::Error::Unsupported(format!(
                "failed to spawn deserialization thread: {e}"
            )))
        })?
        .join()
        .map_err(|_| {
            error::SpannedError::from(error::Error::Unsupported(
                "deserialization thread panicked".to_string(),
            ))
        })?
        .map_err(|e| error::SpannedError::from(error::Error::Unsupported(e)))?;

    if precompiled.version != FORMAT_VERSION {
        return Err(error::SpannedError::from(error::Error::Unsupported(format!(
            "unsupported precompiled format version {} (expected {})",
            precompiled.version, FORMAT_VERSION
        ))));
    }
    load_precompiled(precompiled)
}

/// Compile an XSLT stylesheet into a `PrecompiledStylesheet` bundle.
fn compile_stylesheet(
    stylesheet_path: impl AsRef<Path>,
) -> error::SpannedResult<PrecompiledStylesheet> {
    let stylesheet_path = stylesheet_path.as_ref();
    let xslt = std::fs::read_to_string(stylesheet_path)
        .map_err(|e| error::SpannedError::from(error::Error::Unsupported(format!("read failed: {e}"))))?;

    let canonical = stylesheet_path
        .canonicalize()
        .unwrap_or_else(|_| stylesheet_path.to_path_buf());
    let base_dir = canonical
        .parent()
        .or_else(|| stylesheet_path.parent())
        .map(Path::to_path_buf);
    let static_base_uri: Option<IriAbsoluteString> = format!("file://{}", canonical.display())
        .replace(' ', "%20")
        .try_into()
        .ok();

    let mut static_context = StaticContext::default();
    if let Some(uri) = &static_base_uri {
        static_context.set_static_base_uri(Some(uri.clone()));
    }

    let (ir_declarations, static_context, initial_mode_str) =
        crate::ast_ir::parse_to_ir_with_context(static_context, &xslt, base_dir, None)?;

    let metadata = PrecompiledMetadata {
        namespaces: static_context.namespaces().clone(),
        static_base_uri: static_context.static_base_uri().map(|u| u.to_owned()),
        default_decimal_format: static_context.default_decimal_format().clone(),
        decimal_formats: static_context.decimal_formats().clone(),
        disabled_functions: static_context.disabled_functions().iter().cloned().collect(),
        stylesheet_xslt_version: static_context.stylesheet_xslt_version(),
        processor_xslt_version: static_context.processor_xslt_version(),
        processor_xpath_version: static_context.processor_xpath_version(),
        initial_mode: initial_mode_str,
    };

    Ok(PrecompiledStylesheet {
        version: FORMAT_VERSION,
        metadata,
        declarations: ir_declarations,
    })
}

/// Load a `PrecompiledStylesheet` and compile it to a runnable `Program`.
fn load_precompiled(
    precompiled: PrecompiledStylesheet,
) -> error::SpannedResult<xee_interpreter::interpreter::Program> {
    let static_context = precompiled.metadata.to_static_context();
    let mut program = compile_xslt(precompiled.declarations, static_context)?;
    program.set_dynamic_xpath_evaluator(Box::new(XsltDynamicXPathEvaluator::default()));
    program.set_transform_evaluator(Box::new(crate::transform::XsltTransformEvaluator));
    Ok(program)
}
