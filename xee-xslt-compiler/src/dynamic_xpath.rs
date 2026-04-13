use ahash::HashSetExt;
use xee_interpreter::{
    context::{self, DynamicContext},
    error, function,
    interpreter::{DynamicXPathEvaluator, DynamicXPathRequest, Interpreter},
    sequence,
};
use xee_name::{Name, Namespaces, VariableNames};
use xot::Xot;

#[derive(Debug, Default)]
pub(crate) struct XsltDynamicXPathEvaluator;

impl DynamicXPathEvaluator for XsltDynamicXPathEvaluator {
    fn evaluate(
        &self,
        request: &DynamicXPathRequest,
        context: &DynamicContext,
        interpreter: &mut Interpreter<'_>,
    ) -> error::SpannedResult<sequence::Sequence> {
        let namespaces = namespaces_for_request(
            context,
            request.namespace_context.as_ref(),
            interpreter.xot_mut(),
        )?;
        let (variables, variable_names) = variables_for_request(request.with_params.as_ref())?;
        let static_context = context
            .static_context()
            .clone_with_namespaces_and_variables(namespaces, variable_names);
        let xpath = static_context
            .parse_xpath(&request.xpath)
            .map_err(|error| error::SpannedError {
                error: error::Error::Unsupported(format!("{error:?}")),
                span: None,
            })?;
        let mut program = xee_xpath_compiler::compile(static_context, xpath)?;
        program.set_source(request.xpath.clone());

        let context_item = request
            .context_item
            .clone()
            .or_else(|| context.context_item().cloned());
        let dynamic_context = context.clone_for_program(&program, context_item, variables);
        program
            .runnable(&dynamic_context)
            .many(interpreter.xot_mut())
    }
}

fn variables_for_request(
    with_params: Option<&function::Map>,
) -> error::Result<(context::Variables, VariableNames)> {
    let mut variables = context::Variables::new();
    let mut variable_names = VariableNames::new();
    let Some(with_params) = with_params else {
        return Ok((variables, variable_names));
    };

    for (key, value) in with_params.entries() {
        let name: Name = key.clone().try_into()?;
        variable_names.insert(name.clone());
        variables.insert(name, value.clone());
    }

    Ok((variables, variable_names))
}

fn namespaces_for_request(
    context: &DynamicContext,
    namespace_context: Option<&sequence::Item>,
    xot: &mut Xot,
) -> error::Result<Namespaces> {
    match namespace_context {
        None => Ok(context.static_context().namespaces().clone()),
        Some(sequence::Item::Node(node)) => Ok(namespaces_for_node(*node, xot)),
        Some(_) => Err(error::Error::type_error(
            "xsl:evaluate namespace-context must be a node",
        )),
    }
}

fn namespaces_for_node(node: xot::Node, xot: &Xot) -> Namespaces {
    let mut namespaces = Namespaces::default();
    for (prefix_id, namespace_id) in xot.namespaces_in_scope(node) {
        let prefix = xot.prefix_str(prefix_id);
        let namespace = xot.namespace_str(namespace_id);
        if prefix.is_empty() {
            namespaces.default_element_namespace = namespace.to_string();
        } else {
            namespaces.add(&[(prefix, namespace)]);
        }
    }
    namespaces
}
