use std::path::PathBuf;

use crate::common::input_xml;
use crate::error::{render_error, render_program_error};
use anyhow::Context;
use clap::Parser;
use xot::Xot;

#[derive(Debug, Parser)]
pub(crate) struct Xslt {
    /// XSLT stylesheet file
    pub(crate) stylesheet: PathBuf,

    /// Input XML file (or use stdin if not provided)
    pub(crate) infile: Option<PathBuf>,

    /// Output file (default stdout)
    #[arg(long, short)]
    pub(crate) output: Option<PathBuf>,

    /// Dump the intermediate representation (IR) instead of transforming
    #[arg(long)]
    pub(crate) dump_ir: bool,
}

impl Xslt {
    pub(crate) fn run(&self) -> anyhow::Result<()> {
        // Read the XSLT stylesheet
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

        // Read the input XML
        let xml = input_xml(&self.infile)?;

        // Compile the stylesheet
        let program =
            match xee_xslt_compiler::parse_with_stylesheet_path(&stylesheet, &self.stylesheet) {
                Ok(program) => program,
                Err(e) => {
                    render_error(&self.stylesheet.display().to_string(), &stylesheet, e);
                    return Ok(());
                }
            };

        // Get serialization parameters from xsl:output
        let serialization_params = program.declarations.serialization_params.clone();

        // Perform the XSLT transformation
        let mut xot = Xot::new();
        let root = xot
            .parse(&xml)
            .map_err(|e| anyhow::anyhow!("Failed to parse input XML: {}", e))?;
        let result = match xee_xslt_compiler::evaluate_program(&mut xot, &program, root) {
            Ok(result) => result,
            Err(e) => {
                render_program_error(
                    &program,
                    &self.stylesheet.display().to_string(),
                    &stylesheet,
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
}
