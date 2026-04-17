use std::fs;
use std::path::PathBuf;

use xee_interpreter::{
    atomic::{self, StringType},
    context::DynamicContext,
    error, function,
    interpreter::{Interpreter, TransformEvaluator, TransformRequest},
    sequence,
};

#[derive(Debug, Default)]
pub(crate) struct XsltTransformEvaluator;

impl TransformEvaluator for XsltTransformEvaluator {
    fn transform(
        &self,
        request: &TransformRequest,
        context: &DynamicContext,
        interpreter: &mut Interpreter<'_>,
    ) -> error::SpannedResult<function::Map> {
        let options = &request.options;

        // Extract stylesheet-location (optional — alternatives exist per spec)
        let stylesheet_location =
            get_optional_string_option(options, "stylesheet-location", interpreter.xot())?;

        // If no stylesheet-location, check for unsupported alternatives and
        // report FOXT0002 (cannot locate stylesheet)
        let stylesheet_location = match stylesheet_location {
            Some(loc) => loc,
            None => {
                return Err(error::SpannedError {
                    error: error::Error::FOXT0002,
                    span: None,
                    detail: None,
                    contexts: Vec::new(),
                });
            }
        };

        // Extract optional source-node
        let source_node = get_node_option(options, "source-node")?;

        // Extract optional stylesheet-params
        let stylesheet_params = get_map_option(options, "stylesheet-params")?;

        // Resolve stylesheet path against static-base-uri
        let stylesheet_path = resolve_stylesheet_path(context, &stylesheet_location)?;

        // Read and compile the stylesheet
        let xslt_source =
            fs::read_to_string(&stylesheet_path).map_err(|_| error::SpannedError {
                error: error::Error::FOXT0002,
                span: None,
            detail: None,

            contexts: Vec::new(),
            })?;

        let mut program = crate::run::parse_with_stylesheet_path(&xslt_source, &stylesheet_path)?;
        // Inject evaluators so nested transforms work
        program
            .set_dynamic_xpath_evaluator(Box::new(crate::dynamic_xpath::XsltDynamicXPathEvaluator));
        program.set_transform_evaluator(Box::new(XsltTransformEvaluator));

        // Build variables from stylesheet-params
        let variables = build_variables(stylesheet_params.as_ref())?;

        // Set up documents — share the existing document pool
        let xot = interpreter.xot_mut();

        // Add source-node to documents if provided
        let context_item = if let Some(source) = source_node {
            Some(sequence::Item::Node(source))
        } else {
            None
        };

        let dynamic_context = context.clone_for_program(&program, context_item, variables);

        // Run the transformation
        let result = program.runnable(&dynamic_context).many(xot)?;

        // Normalize the principal output into a document node.
        // Per XPath 3.1 spec, fn:transform()?output must be a document-node().
        let document = result.normalize(" ", xot).map_err(|e| error::SpannedError {
            error: e,
            span: None,
        detail: None,

        contexts: Vec::new(),
        })?;
        let principal_output = sequence::Sequence::from(sequence::Item::Node(document));

        // Build the result map
        build_result_map(principal_output, &dynamic_context)
    }
}

fn get_optional_string_option(
    options: &function::Map,
    key: &str,
    xot: &xot::Xot,
) -> error::SpannedResult<Option<String>> {
    let key_atomic = atomic::Atomic::String(StringType::String, key.into());
    let Some(value) = options.get(&key_atomic) else {
        return Ok(None);
    };
    value
        .string_value(xot)
        .map(Some)
        .map_err(|e| error::SpannedError {
            error: e,
            span: None,
            detail: None,
            contexts: Vec::new(),
        })
}

fn get_node_option(options: &function::Map, key: &str) -> error::SpannedResult<Option<xot::Node>> {
    let key_atomic = atomic::Atomic::String(StringType::String, key.into());
    let Some(value) = options.get(&key_atomic) else {
        return Ok(None);
    };
    let item = value.clone().one().map_err(|e| error::SpannedError {
        error: e,
        span: None,
    detail: None,

    contexts: Vec::new(),
    })?;
    match item {
        sequence::Item::Node(node) => Ok(Some(node)),
        _ => Err(error::SpannedError {
            error: error::Error::type_error(format!(
                "fn:transform: option '{}' must be a node",
                key
            )),
            span: None,
        detail: None,

        contexts: Vec::new(),
        }),
    }
}

fn get_map_option(
    options: &function::Map,
    key: &str,
) -> error::SpannedResult<Option<function::Map>> {
    let key_atomic = atomic::Atomic::String(StringType::String, key.into());
    let Some(value) = options.get(&key_atomic) else {
        return Ok(None);
    };
    let item = value.clone().option().map_err(|e| error::SpannedError {
        error: e,
        span: None,
    detail: None,

    contexts: Vec::new(),
    })?;
    let Some(item) = item else {
        return Ok(None);
    };
    match item {
        sequence::Item::Function(function::Function::Map(map)) => Ok(Some(map)),
        _ => Err(error::SpannedError {
            error: error::Error::type_error(format!(
                "fn:transform: option '{}' must be a map",
                key
            )),
            span: None,
        detail: None,

        contexts: Vec::new(),
        }),
    }
}

fn resolve_stylesheet_path(
    context: &DynamicContext,
    location: &str,
) -> error::SpannedResult<PathBuf> {
    // Handle file:// URIs directly
    if let Some(file_path) = location.strip_prefix("file://") {
        let path = PathBuf::from(file_path);
        if path.exists() {
            return Ok(path);
        }
    }

    // Try to resolve against static-base-uri
    if let Some(base_uri) = context.static_context().static_base_uri() {
        let base_str = base_uri.as_str();
        if let Some(base_path) = base_str.strip_prefix("file://") {
            let base_path = std::path::Path::new(base_path);
            if let Some(base_dir) = base_path.parent() {
                let resolved = base_dir.join(location);
                if resolved.exists() {
                    return Ok(resolved);
                }
            }
        }
    }

    // Try as absolute path
    let path = PathBuf::from(location);
    if path.exists() {
        return Ok(path);
    }

    Err(error::SpannedError {
        error: error::Error::FOXT0002,
        span: None,
    detail: None,

    contexts: Vec::new(),
    })
}

fn build_variables(
    params: Option<&function::Map>,
) -> error::SpannedResult<xee_interpreter::context::Variables> {
    let mut variables = xee_interpreter::context::Variables::new();
    let Some(params) = params else {
        return Ok(variables);
    };

    for (key, value) in params.entries() {
        let name: xee_name::Name = key.clone().try_into().map_err(|_| error::SpannedError {
            error: error::Error::type_error("fn:transform: stylesheet-params key must be a QName"),
            span: None,
        detail: None,

        contexts: Vec::new(),
        })?;
        variables.insert(name, value.clone());
    }

    Ok(variables)
}

fn build_result_map(
    principal_output: sequence::Sequence,
    context: &DynamicContext,
) -> error::SpannedResult<function::Map> {
    let mut entries: Vec<(atomic::Atomic, sequence::Sequence)> = Vec::new();

    // Add principal output as 'output'
    entries.push((
        atomic::Atomic::String(StringType::String, "output".into()),
        principal_output,
    ));

    // Add secondary result documents
    let secondary = context.secondary_result_documents();
    for (href, sequence) in secondary {
        entries.push((
            atomic::Atomic::String(StringType::String, href.into()),
            sequence,
        ));
    }

    function::Map::new(entries).map_err(|e| error::SpannedError {
        error: e,
        span: None,
    detail: None,

    contexts: Vec::new(),
    })
}
