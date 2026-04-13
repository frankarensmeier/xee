use ahash::{AHashMap, HashMap, HashMapExt};
use iri_string::types::{IriStr, IriString};
use std::{cell::RefCell, fmt::Debug};

use crate::declaration::OnMultipleMatch;
use crate::function::{self, Function};
use crate::{error::Error, interpreter::Program};
use crate::{interpreter, sequence};

use super::{DocumentsRef, StaticContext};

/// A map of variables
///
/// These are variables to be passed into an XPath evaluation.
///
/// The key is the name of a variable, and the value is an item.
pub type Variables = AHashMap<xot::xmlname::OwnedName, sequence::Sequence>;

// a dynamic context is created for each xpath evaluation
#[derive(Debug)]
pub struct DynamicContext<'a> {
    // we keep a reference to the program
    program: &'a Program,

    /// An optional context item
    context_item: Option<sequence::Item>,
    // we want to mutate documents during evaluation, and this happens in
    // multiple spots. We use RefCell to manage that during runtime so we don't
    // need to make the whole thing immutable.
    documents: DocumentsRef,
    variables: Variables,
    // TODO: we want to be able to control the creation of this outside,
    // as it needs to be the same for all evalutions of XSLT I believe
    current_datetime: chrono::DateTime<chrono::offset::FixedOffset>,
    // default collection
    default_collection: Option<sequence::Sequence>,
    // collections
    collections: HashMap<IriString, sequence::Sequence>,
    // default uri collection
    default_uri_collection: Option<sequence::Sequence>,
    // uri collections
    uri_collections: HashMap<IriString, sequence::Sequence>,
    // environment variables
    environment_variables: HashMap<String, String>,
    secondary_result_documents: RefCell<HashMap<String, sequence::Sequence>>,
    principal_result_documents: RefCell<Vec<sequence::Sequence>>,
    principal_result_document_parameters: RefCell<Vec<sequence::SerializationParameters>>,
    on_multiple_match: OnMultipleMatch,
}

impl<'a> DynamicContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        program: &'a Program,
        context_item: Option<sequence::Item>,
        documents: DocumentsRef,
        variables: Variables,
        current_datetime: chrono::DateTime<chrono::offset::FixedOffset>,
        default_collection: Option<sequence::Sequence>,
        collections: HashMap<IriString, sequence::Sequence>,
        default_uri_collection: Option<sequence::Sequence>,
        uri_collections: HashMap<IriString, sequence::Sequence>,
        environment_variables: HashMap<String, String>,
        secondary_result_documents: HashMap<String, sequence::Sequence>,
        principal_result_documents: Vec<sequence::Sequence>,
        principal_result_document_parameters: Vec<sequence::SerializationParameters>,
        on_multiple_match: OnMultipleMatch,
    ) -> Self {
        Self {
            program,
            context_item,
            documents,
            variables,
            current_datetime,
            default_collection,
            collections,
            default_uri_collection,
            uri_collections,
            environment_variables,
            secondary_result_documents: RefCell::new(secondary_result_documents),
            principal_result_documents: RefCell::new(principal_result_documents),
            principal_result_document_parameters: RefCell::new(
                principal_result_document_parameters,
            ),
            on_multiple_match,
        }
    }

    /// The static context of the program.
    pub fn static_context(&self) -> &StaticContext {
        self.program.static_context()
    }

    /// Access the context item, if any.
    pub fn context_item(&self) -> Option<&sequence::Item> {
        self.context_item.as_ref()
    }

    /// The documents in this context.
    pub fn documents(&self) -> DocumentsRef {
        self.documents.clone()
    }

    /// The variables in this context.
    pub fn variables(&self) -> &Variables {
        &self.variables
    }

    /// Access the default collection
    pub fn default_collection(&self) -> Option<&sequence::Sequence> {
        self.default_collection.as_ref()
    }

    /// Access a collection by URI
    pub fn collection(&self, uri: &IriStr) -> Option<&sequence::Sequence> {
        self.collections.get(uri)
    }

    /// Access the default URI collection
    pub fn default_uri_collection(&self) -> Option<&sequence::Sequence> {
        self.default_uri_collection.as_ref()
    }

    /// Access a URI collection by URI
    ///
    /// Note that the URI does not have to be a proper URI as the specification
    /// defines it as an xs:string
    pub fn uri_collection(&self, uri: &IriStr) -> Option<&sequence::Sequence> {
        self.uri_collections.get(uri)
    }

    /// Access an environment variable by name
    pub fn environment_variable(&self, name: &str) -> Option<&str> {
        self.environment_variables.get(name).map(String::as_str)
    }

    /// Access all environment variable names
    pub fn environment_variable_names(&self) -> impl Iterator<Item = &str> {
        self.environment_variables.keys().map(String::as_str)
    }

    pub fn store_secondary_result_document(&self, href: String, sequence: sequence::Sequence) {
        self.secondary_result_documents
            .borrow_mut()
            .insert(href, sequence);
    }

    pub fn secondary_result_document(&self, href: &str) -> Option<sequence::Sequence> {
        self.secondary_result_documents.borrow().get(href).cloned()
    }

    pub fn store_principal_result_document(
        &self,
        sequence: sequence::Sequence,
        parameters: sequence::SerializationParameters,
    ) {
        self.principal_result_documents.borrow_mut().push(sequence);
        self.principal_result_document_parameters
            .borrow_mut()
            .push(parameters);
    }

    pub fn principal_result_documents(&self) -> Vec<sequence::Sequence> {
        self.principal_result_documents.borrow().clone()
    }

    pub fn principal_result_document_parameters(
        &self,
    ) -> Option<sequence::SerializationParameters> {
        self.principal_result_document_parameters
            .borrow()
            .last()
            .cloned()
    }

    pub fn serialization_parameters(&self) -> &sequence::SerializationParameters {
        &self.program.declarations.serialization_params
    }

    pub fn on_multiple_match(&self) -> OnMultipleMatch {
        self.on_multiple_match
    }

    pub fn clone_for_program<'b>(
        &self,
        program: &'b Program,
        context_item: Option<sequence::Item>,
        variables: Variables,
    ) -> DynamicContext<'b> {
        DynamicContext::new(
            program,
            context_item,
            self.documents.clone(),
            variables,
            self.current_datetime,
            self.default_collection.clone(),
            self.collections.clone(),
            self.default_uri_collection.clone(),
            self.uri_collections.clone(),
            self.environment_variables.clone(),
            HashMap::new(),
            Vec::new(),
            Vec::new(),
            self.on_multiple_match,
        )
    }

    pub fn dynamic_xpath_evaluator(&self) -> Option<&dyn interpreter::DynamicXPathEvaluator> {
        self.program.dynamic_xpath_evaluator()
    }

    pub fn transform_evaluator(&self) -> Option<&dyn interpreter::TransformEvaluator> {
        self.program.transform_evaluator()
    }

    pub fn secondary_result_documents(&self) -> HashMap<String, sequence::Sequence> {
        self.secondary_result_documents.borrow().clone()
    }

    pub(crate) fn arguments(&self) -> Result<Vec<sequence::Sequence>, Error> {
        let mut arguments = Vec::new();
        for variable_name in self.static_context().variable_names() {
            let items = self.variables.get(variable_name).ok_or(Error::XPDY0002)?;
            arguments.push(items.clone());
        }
        Ok(arguments)
    }

    fn create_current_datetime() -> chrono::DateTime<chrono::offset::FixedOffset> {
        chrono::offset::Local::now().into()
    }

    pub(crate) fn current_datetime(&self) -> chrono::DateTime<chrono::offset::FixedOffset> {
        self.current_datetime
    }

    pub fn implicit_timezone(&self) -> chrono::FixedOffset {
        self.current_datetime.timezone()
    }

    /// Access information about a Function.
    pub fn function_info<'b>(&self, function: &'b Function) -> interpreter::FunctionInfo<'a, 'b> {
        self.program.function_info(function)
    }

    pub(crate) fn static_function_by_id(
        &self,
        id: function::StaticFunctionId,
    ) -> &function::StaticFunction {
        self.program.static_context().function_by_id(id)
    }

    pub(crate) fn inline_function_by_id(
        &self,
        id: function::InlineFunctionId,
    ) -> &function::InlineFunction {
        self.program.inline_function(id)
    }
}
