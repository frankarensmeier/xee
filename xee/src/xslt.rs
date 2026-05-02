use std::path::PathBuf;

use crate::common::input_xml;
use crate::error::{render_error, render_program_error};
use anyhow::Context;
use clap::Parser;
use xot::{ParseOptions, Xot};

#[derive(Debug, Parser)]
pub(crate) struct Xslt {
    /// XSLT stylesheet file (or precompiled .xeec file with --precompiled)
    pub(crate) stylesheet: PathBuf,

    /// Input XML file (or use stdin if not provided)
    pub(crate) infile: Option<PathBuf>,

    /// Output file (default stdout)
    #[arg(long, short)]
    pub(crate) output: Option<PathBuf>,

    /// Stylesheet parameters as name=value pairs (all values supplied as
    /// xs:untypedAtomic; the stylesheet's as= declarations handle casting).
    /// May be repeated: --param foo=bar --param n=42
    #[arg(long = "param", value_name = "NAME=VALUE")]
    pub(crate) params: Vec<String>,

    /// Read stylesheet parameters from a JSON file. The file must contain a
    /// single JSON object whose keys are parameter names and whose values are
    /// strings. All values are supplied as xs:untypedAtomic.
    #[arg(long = "params-file", value_name = "FILE")]
    pub(crate) params_file: Option<PathBuf>,

    /// Compile the stylesheet to a precompiled .xeec file and exit.
    /// The output path is specified with --output (default: <stylesheet>.xeec).
    #[arg(long, conflicts_with = "precompiled")]
    pub(crate) compile: bool,

    /// Load a precompiled .xeec stylesheet instead of a source .xsl file.
    /// Skips the expensive preprocessing step (~94% of compilation time).
    #[arg(long, conflicts_with = "compile")]
    pub(crate) precompiled: bool,

    /// Dump the intermediate representation (IR) instead of transforming
    #[arg(long)]
    pub(crate) dump_ir: bool,

    /// Accept documents with duplicate xml:id values
    #[arg(long)]
    pub(crate) relaxed: bool,
}

impl Xslt {
    pub(crate) fn run(&self) -> anyhow::Result<()> {
        // --compile: precompile stylesheet to .xeec and exit
        if self.compile {
            return self.run_compile();
        }

        // --precompiled: load precompiled .xeec file
        if self.precompiled {
            return self.run_precompiled();
        }

        // Normal mode: read source stylesheet
        let stylesheet = std::fs::read_to_string(&self.stylesheet).with_context(|| {
            format!(
                "Failed to read stylesheet file: {}",
                self.stylesheet.display()
            )
        })?;

        // If --dump-ir, parse to IR and print it
        if self.dump_ir {
            match xee_xslt_compiler::parse_to_ir_with_stylesheet_path(
                &stylesheet,
                &self.stylesheet,
            ) {
                Ok(declarations) => {
                    print!("{}", xee_ir::display::DisplayDeclarations(&declarations));
                }
                Err(e) => {
                    render_error(&self.stylesheet.display().to_string(), &stylesheet, e);
                }
            }
            return Ok(());
        }

        // Compile the stylesheet
        let program =
            match xee_xslt_compiler::parse_with_stylesheet_path(&stylesheet, &self.stylesheet) {
                Ok(program) => program,
                Err(e) => {
                    render_error(&self.stylesheet.display().to_string(), &stylesheet, e);
                    return Ok(());
                }
            };

        self.run_transform(program, &stylesheet)
    }

    /// Compile stylesheet to .xeec file
    fn run_compile(&self) -> anyhow::Result<()> {
        let output_path = self.output.clone().unwrap_or_else(|| {
            self.stylesheet.with_extension("xeec")
        });
        xee_xslt_compiler::precompiled::compile_to_file(&self.stylesheet, &output_path)
            .map_err(|e| anyhow::anyhow!("Compilation failed: {}", e))?;
        eprintln!(
            "Compiled {} -> {}",
            self.stylesheet.display(),
            output_path.display()
        );
        Ok(())
    }

    /// Load precompiled .xeec and run transformation
    fn run_precompiled(&self) -> anyhow::Result<()> {
        let program = xee_xslt_compiler::precompiled::load_from_file(&self.stylesheet)
            .map_err(|e| anyhow::anyhow!("Failed to load precompiled stylesheet: {}", e))?;
        self.run_transform(program, "")
    }

    /// Run an XSLT transformation with the given compiled program.
    /// `fallback_src` is the stylesheet source text for error rendering
    /// (empty for precompiled stylesheets where source is unavailable).
    fn run_transform(&self, program: xee_interpreter::interpreter::Program, fallback_src: &str) -> anyhow::Result<()> {
        // Read the input XML
        let xml = input_xml(&self.infile)?;

        // Get serialization parameters from xsl:output
        let serialization_params = program.declarations.serialization_params.clone();

        // Perform the XSLT transformation
        let mut xot = Xot::new();
        let parse_options = ParseOptions {
            allow_duplicate_ids: self.relaxed,
        };
        let root = xot
            .parse_with_options(&xml, &parse_options)
            .map_err(|e| anyhow::anyhow!("Failed to parse input XML: {}", e))?;

        // Build stylesheet parameters
        let variables = self.build_variables()?;

        let result =
            match xee_xslt_compiler::evaluate_program_with_variables(&mut xot, &program, root, variables) {
            Ok(result) => result,
            Err(e) => {
                render_program_error(
                    &program,
                    &self.stylesheet.display().to_string(),
                    fallback_src,
                    e,
                );
                return Ok(());
            }
        };

        // Convert result to string using xsl:output parameters
        let output_str = result.serialize(serialization_params, &mut xot)?;

        // Output the result
        if let Some(output_path) = &self.output {
            std::fs::write(output_path, output_str).with_context(|| {
                format!("Failed to write output to file: {}", output_path.display())
            })?;
        } else {
            println!("{}", output_str);
        }

        Ok(())
    }

    fn build_variables(&self) -> anyhow::Result<xee_interpreter::context::Variables> {
        use std::rc::Rc;
        let mut variables = xee_interpreter::context::Variables::new();

        // Parse --param NAME=VALUE pairs
        for param in &self.params {
            let (name, value) = param
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("Invalid --param format: '{}'. Expected NAME=VALUE", param))?;
            let owned_name =
                xot::xmlname::OwnedName::new(name.to_string(), String::new(), String::new());
            let atomic = xee_interpreter::atomic::Atomic::Untyped(Rc::from(value));
            let sequence = xee_interpreter::sequence::Sequence::from(
                xee_interpreter::sequence::Item::Atomic(atomic),
            );
            variables.insert(owned_name, sequence);
        }

        // Parse --params-file JSON
        if let Some(params_file) = &self.params_file {
            let content = std::fs::read_to_string(params_file).with_context(|| {
                format!("Failed to read params file: {}", params_file.display())
            })?;
            let json: serde_json::Value =
                serde_json::from_str(&content).with_context(|| {
                    format!("Failed to parse params file as JSON: {}", params_file.display())
                })?;
            let obj = json
                .as_object()
                .ok_or_else(|| anyhow::anyhow!("Params file must contain a JSON object"))?;
            for (name, value) in obj {
                let str_value = match value {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    serde_json::Value::Null => String::new(),
                    _ => anyhow::bail!(
                        "Unsupported value type for parameter '{}': expected string, number, boolean, or null",
                        name
                    ),
                };
                let owned_name =
                    xot::xmlname::OwnedName::new(name.clone(), String::new(), String::new());
                let atomic = xee_interpreter::atomic::Atomic::Untyped(Rc::from(str_value.as_str()));
                let sequence = xee_interpreter::sequence::Sequence::from(
                    xee_interpreter::sequence::Item::Atomic(atomic),
                );
                variables.insert(owned_name, sequence);
            }
        }

        Ok(variables)
    }
}
