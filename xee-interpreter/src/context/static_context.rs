use ahash::{HashMap, HashSet};
use std::cell::RefCell;
use std::fmt::Debug;
use std::rc::Rc;
use std::sync::LazyLock;

use iri_string::types::IriAbsoluteStr;
use iri_string::types::IriAbsoluteString;
use iri_string::types::IriReferenceStr;
use xee_name::{Namespaces, VariableNames};
use xee_xpath_ast::ast;
use xee_xpath_ast::XPathParserContext;
use xot::xmlname::OwnedName;

use crate::error;
use crate::function;
use crate::string::{Collation, Collations};

static STATIC_FUNCTIONS: LazyLock<function::StaticFunctions> =
    LazyLock::new(function::StaticFunctions::new);

// use lazy static to initialize the default collation
static DEFAULT_COLLATION: LazyLock<IriAbsoluteString> = LazyLock::new(|| {
    "http://www.w3.org/2005/xpath-functions/collation/codepoint"
        .try_into()
        .unwrap()
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecimalFormatSymbols {
    pub decimal_separator: char,
    pub grouping_separator: char,
    pub infinity: String,
    pub minus_sign: char,
    pub exponent_separator: char,
    pub nan: String,
    pub percent: char,
    pub per_mille: char,
    pub zero_digit: char,
    pub digit: char,
    pub pattern_separator: char,
}

impl Default for DecimalFormatSymbols {
    fn default() -> Self {
        Self {
            decimal_separator: '.',
            grouping_separator: ',',
            infinity: "Infinity".to_string(),
            minus_sign: '-',
            exponent_separator: 'e',
            nan: "NaN".to_string(),
            percent: '%',
            per_mille: '\u{2030}',
            zero_digit: '0',
            digit: '#',
            pattern_separator: ';',
        }
    }
}

#[derive(Debug)]
pub struct StaticContext {
    parser_context: XPathParserContext,
    functions: &'static function::StaticFunctions,
    disabled_functions: HashSet<xot::xmlname::OwnedName>,
    // TODO: try to make collations static
    collations: RefCell<Collations>,
    static_base_uri: Option<IriAbsoluteString>,
    default_decimal_format: DecimalFormatSymbols,
    decimal_formats: HashMap<OwnedName, DecimalFormatSymbols>,
    stylesheet_xslt_version: Option<u8>,
    processor_xslt_version: Option<u8>,
    processor_xpath_version: Option<u8>,
}

impl Default for StaticContext {
    fn default() -> Self {
        Self::new(
            Namespaces::default(),
            VariableNames::default(),
            HashSet::default(),
            None,
        )
    }
}

impl From<XPathParserContext> for StaticContext {
    fn from(parser_context: XPathParserContext) -> Self {
        Self {
            parser_context,
            functions: &STATIC_FUNCTIONS,
            disabled_functions: HashSet::default(),
            collations: RefCell::new(Collations::new()),
            static_base_uri: None,
            default_decimal_format: DecimalFormatSymbols::default(),
            decimal_formats: HashMap::default(),
            stylesheet_xslt_version: None,
            processor_xslt_version: None,
            processor_xpath_version: None,
        }
    }
}

impl StaticContext {
    pub(crate) fn new(
        namespaces: Namespaces,
        variable_names: VariableNames,
        disabled_functions: HashSet<xot::xmlname::OwnedName>,
        static_base_uri: Option<IriAbsoluteString>,
    ) -> Self {
        Self {
            parser_context: XPathParserContext::new(namespaces, variable_names),
            functions: &STATIC_FUNCTIONS,
            disabled_functions,
            collations: RefCell::new(Collations::new()),
            static_base_uri,
            default_decimal_format: DecimalFormatSymbols::default(),
            decimal_formats: HashMap::default(),
            stylesheet_xslt_version: None,
            processor_xslt_version: None,
            processor_xpath_version: None,
        }
    }

    pub fn from_namespaces(namespaces: Namespaces) -> Self {
        Self::new(
            namespaces,
            VariableNames::default(),
            HashSet::default(),
            None,
        )
    }

    pub fn clone_with_namespaces(&self, namespaces: Namespaces) -> Self {
        Self {
            parser_context: XPathParserContext::new(
                namespaces,
                self.parser_context.variable_names.clone(),
            ),
            functions: self.functions,
            disabled_functions: self.disabled_functions.clone(),
            collations: RefCell::new(Collations::new()),
            static_base_uri: self.static_base_uri.clone(),
            default_decimal_format: self.default_decimal_format.clone(),
            decimal_formats: self.decimal_formats.clone(),
            stylesheet_xslt_version: self.stylesheet_xslt_version,
            processor_xslt_version: self.processor_xslt_version,
            processor_xpath_version: self.processor_xpath_version,
        }
    }

    pub fn clone_with_namespaces_and_variables(
        &self,
        namespaces: Namespaces,
        variable_names: VariableNames,
    ) -> Self {
        Self {
            parser_context: XPathParserContext::new(namespaces, variable_names),
            functions: self.functions,
            disabled_functions: self.disabled_functions.clone(),
            collations: RefCell::new(Collations::new()),
            static_base_uri: self.static_base_uri.clone(),
            default_decimal_format: self.default_decimal_format.clone(),
            decimal_formats: self.decimal_formats.clone(),
            stylesheet_xslt_version: self.stylesheet_xslt_version,
            processor_xslt_version: self.processor_xslt_version,
            processor_xpath_version: self.processor_xpath_version,
        }
    }

    pub fn clone_with_static_base_uri(&self, static_base_uri: Option<IriAbsoluteString>) -> Self {
        Self {
            parser_context: XPathParserContext::new(
                self.parser_context.namespaces.clone(),
                self.parser_context.variable_names.clone(),
            ),
            functions: self.functions,
            disabled_functions: self.disabled_functions.clone(),
            collations: RefCell::new(Collations::new()),
            static_base_uri,
            default_decimal_format: self.default_decimal_format.clone(),
            decimal_formats: self.decimal_formats.clone(),
            stylesheet_xslt_version: self.stylesheet_xslt_version,
            processor_xslt_version: self.processor_xslt_version,
            processor_xpath_version: self.processor_xpath_version,
        }
    }

    pub fn namespaces(&self) -> &Namespaces {
        &self.parser_context.namespaces
    }

    pub fn variable_names(&self) -> &VariableNames {
        &self.parser_context.variable_names
    }

    pub fn default_decimal_format(&self) -> &DecimalFormatSymbols {
        &self.default_decimal_format
    }

    pub fn decimal_format(&self, name: Option<&OwnedName>) -> Option<&DecimalFormatSymbols> {
        match name {
            Some(name) => self.decimal_formats.get(name),
            None => Some(&self.default_decimal_format),
        }
    }

    pub fn set_decimal_formats(
        &mut self,
        default_decimal_format: DecimalFormatSymbols,
        decimal_formats: HashMap<OwnedName, DecimalFormatSymbols>,
    ) {
        self.default_decimal_format = default_decimal_format;
        self.decimal_formats = decimal_formats;
    }

    pub fn stylesheet_xslt_version(&self) -> Option<u8> {
        self.stylesheet_xslt_version
    }

    /// Returns true if the stylesheet is in backwards-compatible mode
    /// (stylesheet version < processor version, typically version="1.0").
    pub fn backwards_compatible(&self) -> bool {
        match (self.stylesheet_xslt_version, self.processor_xslt_version) {
            (Some(sv), Some(pv)) => sv < pv,
            _ => false,
        }
    }

    pub fn set_stylesheet_xslt_version(&mut self, xslt_version: Option<u8>) {
        self.stylesheet_xslt_version = xslt_version;
    }

    pub fn processor_xslt_version(&self) -> Option<u8> {
        self.processor_xslt_version
    }

    pub fn set_processor_xslt_version(&mut self, xslt_version: Option<u8>) {
        self.processor_xslt_version = xslt_version;
    }

    pub fn processor_xpath_version(&self) -> Option<u8> {
        self.processor_xpath_version
    }

    pub fn set_processor_xpath_version(&mut self, xpath_version: Option<u8>) {
        self.processor_xpath_version = xpath_version;
    }

    pub fn default_collation(&self) -> error::Result<Rc<Collation>> {
        self.collation(self.default_collation_uri())
    }

    pub fn default_collation_uri(&self) -> &IriReferenceStr {
        DEFAULT_COLLATION.as_ref()
    }

    pub(crate) fn resolve_collation_str(
        &self,
        collation: Option<&str>,
    ) -> error::Result<Rc<Collation>> {
        let collation: Option<&IriReferenceStr> = if let Some(collation) = collation {
            collation.try_into().ok()
        } else {
            None
        };
        self.collation(collation.unwrap_or(self.default_collation_uri()))
    }

    pub fn static_base_uri(&self) -> Option<&IriAbsoluteStr> {
        self.static_base_uri.as_deref()
    }

    pub(crate) fn collation(&self, uri: &IriReferenceStr) -> error::Result<Rc<Collation>> {
        self.collations
            .borrow_mut()
            .load(self.static_base_uri(), uri)
    }

    /// Given an XPath string, parse into an XPath AST
    ///
    /// This uses the namespaces and variable names with which
    /// this static context has been initialized.
    pub fn parse_xpath(&self, s: &str) -> Result<ast::XPath, xee_xpath_ast::ParserError> {
        self.parser_context.parse_xpath(s)
    }

    /// Parse an XPath string as it would appear in an XSLT value template.
    /// This means it should have a closing `}` following the xpath expression.
    pub fn parse_value_template_xpath(
        &self,
        s: &str,
    ) -> Result<ast::XPath, xee_xpath_ast::ParserError> {
        self.parser_context.parse_value_template_xpath(s)
    }

    /// Get a static function by id
    pub fn function_by_id(
        &self,
        static_function_id: function::StaticFunctionId,
    ) -> &function::StaticFunction {
        self.functions.get_by_index(static_function_id)
    }

    /// Get a static function by name and arity
    pub fn function_id_by_name(
        &self,
        name: &xot::xmlname::OwnedName,
        arity: u8,
    ) -> Option<function::StaticFunctionId> {
        if self.disabled_functions.contains(name) {
            return None;
        }
        self.functions.get_by_name(name, arity)
    }

    pub fn has_function_name(&self, name: &xot::xmlname::OwnedName) -> bool {
        if self.disabled_functions.contains(name) {
            return false;
        }
        self.functions.has_by_name(name)
    }

    pub fn disable_function(&mut self, name: xot::xmlname::OwnedName) {
        self.disabled_functions.insert(name);
    }

    pub fn is_function_disabled(&self, name: &xot::xmlname::OwnedName) -> bool {
        self.disabled_functions.contains(name)
    }

    /// Get an internal static function by name and arity
    pub fn function_id_by_internal_name(
        &self,
        name: &xot::xmlname::OwnedName,
        arity: u8,
    ) -> Option<function::StaticFunctionId> {
        self.functions.get_by_internal_name(name, arity)
    }
}
