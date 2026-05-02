mod ast_ir;
mod dynamic_xpath;
#[cfg(feature = "serde")]
pub mod precompiled;
mod priority;
mod run;
mod transform;

pub use ast_ir::{parse, parse_to_ir, parse_to_ir_with_context, parse_with_base_dir, parse_with_base_dir_and_initial_mode};
pub use run::{
    evaluate, evaluate_program, evaluate_program_with_variables, evaluate_with_base_dir,
    evaluate_with_stylesheet_path, parse_to_ir_with_stylesheet_path, parse_with_stylesheet_path,
};
