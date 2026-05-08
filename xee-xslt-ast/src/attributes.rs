use std::str::FromStr;

use ahash::{HashSet, HashSetExt};
use rust_decimal::Decimal;
use xee_xpath_ast::{ast as xpath_ast, parse_item_type, parse_name, parse_sequence_type};
use xot::xmlname::NameStrInfo;

use crate::ast_core as ast;
use crate::content::Content;
use crate::error::AttributeError;
use crate::name::XmlName;
use crate::names::StandardNames;
use crate::tokenize::split_whitespace_with_spans;
use crate::{ast_core::Span, value_template::ValueTemplateTokenizer};
use xot::{NameId, SpanInfoKey};

fn has_rooted_pattern(pattern: &xee_xpath_ast::Pattern<xpath_ast::ExprS>) -> bool {
    match pattern {
        xee_xpath_ast::Pattern::Predicate(_) => false,
        xee_xpath_ast::Pattern::Expr(expr_pattern) => has_rooted_expr_pattern(expr_pattern),
    }
}

fn has_rooted_expr_pattern(
    expr_pattern: &xee_xpath_ast::pattern::ExprPattern<xpath_ast::ExprS>,
) -> bool {
    match expr_pattern {
        xee_xpath_ast::pattern::ExprPattern::Path(path_expr) => {
            matches!(
                path_expr.root,
                xee_xpath_ast::pattern::PathRoot::Rooted { .. }
            )
        }
        xee_xpath_ast::pattern::ExprPattern::BinaryExpr(binary_expr) => {
            has_rooted_expr_pattern(&binary_expr.left)
                || has_rooted_expr_pattern(&binary_expr.right)
        }
    }
}

#[derive(Clone)]
pub(crate) struct Attributes<'a> {
    pub(crate) content: Content<'a>,
    pub(crate) element: &'a xot::Element,
    seen: std::cell::RefCell<HashSet<NameId>>,
}

impl<'a> Attributes<'a> {
    pub(crate) fn new(content: Content<'a>, element: &'a xot::Element) -> Self {
        Self {
            content,
            element,
            seen: std::cell::RefCell::new(HashSet::new()),
        }
    }

    pub(crate) fn with_standard(self) -> Result<Self, AttributeError> {
        // create a new content has a context including standard attributes
        let content = self.content.with_context(
            self.content
                .context
                .with_standard(self.content.xot_namespaces(), self.standard()?),
        );
        // we now create a new attributes object that has the new content
        Ok(Self { content, ..self })
    }

    pub(crate) fn with_static_standard(self) -> Result<Self, AttributeError> {
        // create a new content has a context including standard attributes
        let content = self.content.with_context(
            self.content
                .context
                .with_static_standard(self.content.xot_namespaces(), self.static_standard()?),
        );
        // we now create a new attributes object that has the new content
        Ok(Self { content, ..self })
    }

    pub(crate) fn optional<T>(
        &self,
        name: NameId,
        parse_value: impl Fn(&'a str, Span) -> Result<T, AttributeError>,
    ) -> Result<Option<T>, AttributeError> {
        self.seen.borrow_mut().insert(name);
        if let Some(value) = self.content.xot_attributes().get(name) {
            let span = self.value_span(name)?;
            let value = parse_value(value, span).map_err(|e| {
                if let AttributeError::XPathParser(e) = e {
                    AttributeError::XPathParser(e.adjust(span.start))
                } else {
                    e
                }
            })?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    pub(crate) fn required<T>(
        &self,
        name: NameId,
        parse_value: impl Fn(&'a str, Span) -> Result<T, AttributeError>,
    ) -> Result<T, AttributeError> {
        self.optional(name, parse_value)?.ok_or_else(|| {
            let (local, namespace) = self.content.state.xot.name_ns_str(name);
            let span = match self.span() {
                Ok(span) => span,
                Err(e) => return e,
            };
            AttributeError::NotFound {
                name: XmlName {
                    namespace: namespace.to_string(),
                    local: local.to_string(),
                },
                span,
            }
        })
    }

    pub(crate) fn span(&self) -> Result<Span, AttributeError> {
        self.content
            .state
            .span(self.content.node)
            .ok_or(AttributeError::Internal)
    }

    pub(crate) fn boolean_with_default(
        &self,
        name: NameId,
        default: bool,
    ) -> Result<bool, AttributeError> {
        self.optional(name, Self::_boolean)
            .map(|v| v.unwrap_or(default))
    }

    pub(crate) fn unseen_attributes(&self) -> Vec<NameId> {
        let mut result = Vec::new();
        let seen = self.seen.borrow();
        for name in self.content.state.xot.attributes(self.content.node).keys() {
            if !seen.contains(&name) {
                result.push(name);
            }
        }
        result
    }

    pub(crate) fn validate_unseen(&self) -> Result<(), AttributeError> {
        let unseen_attributes = self
            .unseen_attributes()
            .into_iter()
            .filter(|name| {
                if *name == self.content.state.names.xml_base {
                    return false;
                }
                let namespace = self.content.state.xot.namespace_for_name(*name);
                self.content.state.xot.namespace_str(namespace).is_empty()
                    || namespace == self.content.state.names.xsl_ns
            })
            .collect::<Vec<_>>();
        if !unseen_attributes.is_empty() {
            let name = unseen_attributes[0];
            let namespace = self.content.state.xot.namespace_for_name(name);
            if !self.in_xsl_namespace() && namespace == self.content.state.names.xsl_ns {
                return Err(AttributeError::StaticError {
                    code: "XTSE0805",
                    span: self
                        .content
                        .state
                        .attribute_name_span(self.content.node, name)?,
                });
            }
            return Err(self.content.state.attribute_unexpected(
                self.content.node,
                name,
                "unexpected attribute",
            ));
        }
        Ok(())
    }

    fn trim_token(s: &str) -> &str {
        s.trim()
    }

    fn _boolean(s: &str, span: Span) -> Result<bool, AttributeError> {
        let s = Self::trim_token(s);
        match s {
            "yes" | "true" | "1" => Ok(true),
            "no" | "false" | "0" => Ok(false),
            _ => Err(AttributeError::Invalid {
                value: s.to_string(),
                span,
            }),
        }
    }

    pub(crate) fn boolean(&self) -> impl Fn(&'a str, Span) -> Result<bool, AttributeError> + '_ {
        Self::_boolean
    }

    pub(crate) fn value_template<T>(
        &self,
        _parse_value: impl Fn(&'a str, Span) -> Result<T, AttributeError> + 'a,
    ) -> impl Fn(&'a str, Span) -> Result<ast::ValueTemplate<T>, AttributeError> + '_
    where
        T: Clone + PartialEq + Eq,
    {
        let parser_context = self.content.parser_context();
        move |s, span| {
            let iter = ValueTemplateTokenizer::new(s, span, &parser_context);
            let mut tokens = Vec::new();
            for t in iter {
                let t = t?;
                tokens.push(t.into());
            }

            Ok(ast::ValueTemplate {
                template: tokens,
                phantom: std::marker::PhantomData,
            })
        }
    }

    fn value_span(&self, name: NameId) -> Result<Span, AttributeError> {
        let span = self
            .content
            .state
            .span_info
            .get(SpanInfoKey::AttributeValue(self.content.node, name))
            .ok_or(AttributeError::Internal)?;
        Ok(span.into())
    }

    pub(crate) fn in_xsl_namespace(&self) -> bool {
        self.content
            .state
            .xot
            .namespace_for_name(self.element.name())
            == self.content.state.names.xsl_ns
    }

    pub(crate) fn use_when(&self) -> Result<Option<ast::Expression>, AttributeError> {
        if self.in_xsl_namespace() {
            self.optional(self.content.state.names.standard.use_when, self.xpath())
        } else {
            self.optional(self.content.state.names.xsl_standard.use_when, self.xpath())
        }
    }

    pub(crate) fn standard(&self) -> Result<ast::Standard, AttributeError> {
        if self.in_xsl_namespace() {
            self._standard(&self.content.state.names.standard)
        } else {
            self._standard(&self.content.state.names.xsl_standard)
        }
    }

    pub(crate) fn static_standard(&self) -> Result<ast::StaticStandard, AttributeError> {
        if self.in_xsl_namespace() {
            self._static_standard(&self.content.state.names.standard)
        } else {
            self._static_standard(&self.content.state.names.xsl_standard)
        }
    }

    fn _standard(&self, names: &StandardNames) -> Result<ast::Standard, AttributeError> {
        let xpath_default_namespace = self.optional(names.xpath_default_namespace, self.uri())?;
        let use_when = if let Some(xpath_default_namespace) = &xpath_default_namespace {
            self.seen.borrow_mut().insert(names.use_when);
            let content = self
                .content
                .with_context(self.content.context.with_static_standard(
                    self.content.xot_namespaces(),
                    ast::StaticStandard {
                        xpath_default_namespace: Some(xpath_default_namespace.clone()),
                    },
                ));
            let attributes = Self {
                content,
                element: self.element,
                seen: std::cell::RefCell::new(self.seen.borrow().clone()),
            };
            attributes.optional(names.use_when, attributes.xpath())?
        } else {
            self.optional(names.use_when, self.xpath())?
        };

        Ok(ast::Standard {
            default_collation: self.optional(names.default_collation, self.uris())?,
            default_mode: self.optional(names.default_mode, self.default_mode())?,
            default_validation: self
                .optional(names.default_validation, self.default_validation())?,
            exclude_result_prefixes: self.optional(
                names.exclude_result_prefixes,
                self.exclude_result_prefixes(),
            )?,
            expand_text: self.optional(names.expand_text, self.boolean())?,
            extension_element_prefixes: self
                .optional(names.extension_element_prefixes, self.prefixes())?,
            use_when,
            version: self.optional(names.version, Self::_stylesheet_version_decimal)?,
            xpath_default_namespace,
        })
    }

    fn _static_standard(
        &self,
        names: &StandardNames,
    ) -> Result<ast::StaticStandard, AttributeError> {
        Ok(ast::StaticStandard {
            xpath_default_namespace: self.optional(names.xpath_default_namespace, self.uri())?,
        })
    }

    fn _qname(s: &str, _span: Span) -> Result<ast::QName, AttributeError> {
        Ok(s.to_string())
    }

    pub(crate) fn qname(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::QName, AttributeError> + '_ {
        Self::_qname
    }

    fn _ncname(s: &str, _span: Span) -> Result<ast::NcName, AttributeError> {
        Ok(s.to_string())
    }

    pub(crate) fn ncname(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::NcName, AttributeError> + '_ {
        Self::_ncname
    }

    fn _id(s: &str, _span: Span) -> Result<ast::Id, AttributeError> {
        Ok(s.to_string())
    }

    pub(crate) fn id(&self) -> impl Fn(&'a str, Span) -> Result<ast::Id, AttributeError> + '_ {
        Self::_id
    }

    fn _char(s: &str, span: Span) -> Result<char, AttributeError> {
        let mut chars = s.chars();
        if let Some(char) = chars.next() {
            if chars.next().is_none() {
                return Ok(char);
            }
        }
        Err(AttributeError::Invalid {
            value: s.to_string(),
            span,
        })
    }

    pub(crate) fn char(&self) -> impl Fn(&'a str, Span) -> Result<char, AttributeError> {
        Self::_char
    }

    fn _string(s: &str, _span: Span) -> Result<String, AttributeError> {
        Ok(s.to_string())
    }

    fn _stylesheet_version_decimal(s: &str, span: Span) -> Result<Decimal, AttributeError> {
        let s = Self::trim_token(s);
        Decimal::from_str(s).map_err(|_| AttributeError::StaticError {
            code: "XTSE0110",
            span,
        })
    }

    pub(crate) fn string(&self) -> impl Fn(&'a str, Span) -> Result<String, AttributeError> + '_ {
        Self::_string
    }

    fn _eqname(&self, s: &str, span: Span) -> Result<xpath_ast::Name, AttributeError> {
        let s = Self::trim_token(s);
        if let Ok(name) = parse_name(s, &self.content.parser_context().namespaces).map(|n| n.value)
        {
            Ok(name)
        } else {
            Err(AttributeError::InvalidEqName {
                value: s.to_string(),
                span,
            })
        }
    }

    /// Parse an EqName that represents an element name in an XSLT attribute value.
    /// Unprefixed names use the XML default namespace (xmlns="..."), not
    /// xpath-default-namespace. Used for cdata-section-elements and suppress-indentation.
    fn _element_eqname(&self, s: &str, span: Span) -> Result<xpath_ast::Name, AttributeError> {
        let s = Self::trim_token(s);
        let ctx = self.content.eqname_parser_context();
        if let Ok(name) = parse_name(s, &ctx.namespaces).map(|n| n.value) {
            // parse_name doesn't apply default_element_namespace to unprefixed names
            // (correct for XPath), so we apply it for XSLT element name attributes.
            let name = if name.prefix().is_empty() && name.namespace().is_empty() {
                let default_ns = ctx.namespaces.default_element_namespace();
                if !default_ns.is_empty() {
                    name.with_default_namespace(default_ns)
                } else {
                    name
                }
            } else {
                name
            };
            Ok(name)
        } else {
            Err(AttributeError::InvalidEqName {
                value: s.to_string(),
                span,
            })
        }
    }

    pub(crate) fn eqname(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<xpath_ast::Name, AttributeError> + '_ {
        |s, span| self._eqname(s, span)
    }

    fn _eqnames(&self, s: &str, span: Span) -> Result<Vec<xpath_ast::Name>, AttributeError> {
        let mut result = Vec::new();
        for (s, span) in split_whitespace_with_spans(s, span) {
            result.push(self._eqname(s, span)?);
        }
        Ok(result)
    }

    fn _element_eqnames(
        &self,
        s: &str,
        span: Span,
    ) -> Result<Vec<xpath_ast::Name>, AttributeError> {
        let mut result = Vec::new();
        for (s, span) in split_whitespace_with_spans(s, span) {
            result.push(self._element_eqname(s, span)?);
        }
        Ok(result)
    }

    pub(crate) fn eqnames(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<Vec<xpath_ast::Name>, AttributeError> + '_ {
        |s, span| self._eqnames(s, span)
    }

    /// Parse a list of EqNames that represent element names.
    /// Uses XML default namespace for unprefixed names.
    pub(crate) fn element_eqnames(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<Vec<xpath_ast::Name>, AttributeError> + '_ {
        |s, span| self._element_eqnames(s, span)
    }

    fn _modes(&self, s: &str, span: Span) -> Result<Vec<ast::ModeValue>, AttributeError> {
        let mut result = Vec::new();
        for (s, span) in split_whitespace_with_spans(s, span) {
            match s {
                "#default" => match &self.content.context.default_mode {
                    ast::DefaultMode::Unnamed => {
                        result.push(ast::ModeValue::Unnamed);
                    }
                    ast::DefaultMode::EqName(name) => {
                        result.push(ast::ModeValue::EqName(name.clone()));
                    }
                },
                "#all" => {
                    result.push(ast::ModeValue::All);
                }
                "#unnamed" => {
                    result.push(ast::ModeValue::Unnamed);
                }
                _ => {
                    result.push(ast::ModeValue::EqName(self._eqname(s, span)?));
                }
            }
        }
        Ok(result)
    }

    pub(crate) fn modes(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<Vec<ast::ModeValue>, AttributeError> + '_ {
        |s, span| self._modes(s, span)
    }

    fn _token(s: &str, _span: Span) -> Result<ast::Token, AttributeError> {
        Ok(s.to_string())
    }

    fn _apply_templates_mode(
        &self,
        s: &str,
        span: Span,
    ) -> Result<ast::ApplyTemplatesModeValue, AttributeError> {
        let s = Self::trim_token(s);
        Ok(match s {
            "#default" => match &self.content.context.default_mode {
                ast::DefaultMode::Unnamed => return Ok(ast::ApplyTemplatesModeValue::Unnamed),
                ast::DefaultMode::EqName(name) => {
                    return Ok(ast::ApplyTemplatesModeValue::EqName(name.clone()))
                }
            },
            "#unnamed" => ast::ApplyTemplatesModeValue::Unnamed,
            "#current" => ast::ApplyTemplatesModeValue::Current,
            _ => ast::ApplyTemplatesModeValue::EqName(self._eqname(s, span)?),
        })
    }

    pub(crate) fn apply_templates_mode(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::ApplyTemplatesModeValue, AttributeError> + '_ {
        |s, span| self._apply_templates_mode(s, span)
    }

    pub(crate) fn token(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::Token, AttributeError> + '_ {
        Self::_token
    }

    fn _nmtoken(s: &str, _span: Span) -> Result<ast::NmToken, AttributeError> {
        Ok(s.to_string())
    }

    pub(crate) fn nmtoken(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::NmToken, AttributeError> + '_ {
        Self::_nmtoken
    }

    fn _tokens(s: &str, span: Span) -> Result<Vec<ast::Token>, AttributeError> {
        let mut result = Vec::new();
        for s in s.split_whitespace() {
            result.push(Self::_token(s, span)?);
        }
        Ok(result)
    }

    pub(crate) fn tokens(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<Vec<ast::Token>, AttributeError> + '_ {
        Self::_tokens
    }

    fn _uri(s: &str, _span: Span) -> Result<ast::Uri, AttributeError> {
        // TODO: should actually verify URI?
        Ok(s.to_string())
    }

    pub(crate) fn uri(&self) -> impl Fn(&'a str, Span) -> Result<ast::Uri, AttributeError> + '_ {
        Self::_uri
    }

    fn _uris(s: &str, span: Span) -> Result<Vec<ast::Uri>, AttributeError> {
        let mut result = Vec::new();
        for s in s.split_whitespace() {
            result.push(Self::_uri(s, span)?);
        }
        Ok(result)
    }

    pub(crate) fn uris(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<Vec<ast::Uri>, AttributeError> + '_ {
        Self::_uris
    }

    fn _integer(s: &str, span: Span) -> Result<usize, AttributeError> {
        match s.parse() {
            Ok(i) => Ok(i),
            Err(_) => Err(AttributeError::Invalid {
                value: s.to_string(),
                span,
            }),
        }
    }

    pub(crate) fn integer(&self) -> impl Fn(&'a str, Span) -> Result<usize, AttributeError> + '_ {
        Self::_integer
    }

    fn _default_mode(&self, s: &str, span: Span) -> Result<ast::DefaultMode, AttributeError> {
        let s = Self::trim_token(s);
        if s == "#unnamed" {
            Ok(ast::DefaultMode::Unnamed)
        } else {
            Ok(ast::DefaultMode::EqName(self._eqname(s, span)?))
        }
    }

    pub(crate) fn default_mode(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::DefaultMode, AttributeError> + '_ {
        |s, span| self._default_mode(s, span)
    }

    fn _default_validation(s: &str, span: Span) -> Result<ast::DefaultValidation, AttributeError> {
        match s {
            "preserve" => Ok(ast::DefaultValidation::Preserve),
            "strip" => Ok(ast::DefaultValidation::Strip),
            _ => Err(AttributeError::Invalid {
                value: s.to_string(),
                span,
            }),
        }
    }

    pub(crate) fn default_validation(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::DefaultValidation, AttributeError> + '_ {
        Self::_default_validation
    }

    fn _prefix(s: &str, _span: Span) -> Result<ast::Prefix, AttributeError> {
        // TODO: check whether it's a valid prefix
        Ok(s.to_string())
    }

    pub(crate) fn prefix(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::Prefix, AttributeError> + '_ {
        Self::_prefix
    }

    fn _prefix_or_default(s: &str, span: Span) -> Result<ast::PrefixOrDefault, AttributeError> {
        if s == "#default" {
            Ok(ast::PrefixOrDefault::Default)
        } else {
            Ok(ast::PrefixOrDefault::Prefix(Self::_prefix(s, span)?))
        }
    }

    pub(crate) fn prefix_or_default(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::PrefixOrDefault, AttributeError> + '_ {
        Self::_prefix_or_default
    }

    fn _prefixes(s: &str, span: Span) -> Result<Vec<ast::Prefix>, AttributeError> {
        let mut result = Vec::new();
        for s in s.split_whitespace() {
            result.push(Self::_prefix(s, span)?);
        }
        Ok(result)
    }

    pub(crate) fn prefixes(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<Vec<ast::Prefix>, AttributeError> + '_ {
        Self::_prefixes
    }

    fn _exclude_result_prefix(
        s: &str,
        span: Span,
    ) -> Result<ast::ExcludeResultPrefix, AttributeError> {
        if s == "#default" {
            Ok(ast::ExcludeResultPrefix::Default)
        } else {
            Ok(ast::ExcludeResultPrefix::Prefix(Self::_prefix(s, span)?))
        }
    }

    fn _exclude_result_prefixes(
        s: &str,
        span: Span,
    ) -> Result<ast::ExcludeResultPrefixes, AttributeError> {
        if s == "#all" {
            Ok(ast::ExcludeResultPrefixes::All)
        } else {
            let mut prefixes = Vec::new();
            for s in s.split_whitespace() {
                prefixes.push(Self::_exclude_result_prefix(s, span)?);
            }
            Ok(ast::ExcludeResultPrefixes::Prefixes(prefixes))
        }
    }

    pub(crate) fn exclude_result_prefixes(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::ExcludeResultPrefixes, AttributeError> {
        Self::_exclude_result_prefixes
    }

    fn _xpath(&self, s: &str, span: Span) -> Result<ast::Expression, AttributeError> {
        let namespaces = self.content.context.literal_namespaces(self.content.state);
        match self.content.parser_context().parse_xpath(s) {
            Ok(xpath) => Ok(ast::Expression {
                xpath,
                span,
                namespaces,
            }),
            Err(error) => {
                let Some(rewritten) = rewrite_large_decimal_format_number_call(s) else {
                    return Err(error.into());
                };
                Ok(ast::Expression {
                    xpath: self.content.parser_context().parse_xpath(&rewritten)?,
                    span,
                    namespaces,
                })
            }
        }
    }

    pub(crate) fn xpath(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::Expression, AttributeError> + '_ {
        |s, span| self._xpath(s, span)
    }

    fn _pattern(&self, s: &str, span: Span) -> Result<ast::Pattern, AttributeError> {
        let pattern = self.content.parser_context().parse_pattern(s)?;
        if has_rooted_pattern(&pattern) && !self.content.context.supports_rooted_patterns() {
            return Err(AttributeError::StaticError {
                code: "XTSE0340",
                span,
            });
        }
        Ok(ast::Pattern { pattern, span })
    }

    pub(crate) fn pattern(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::Pattern, AttributeError> + '_ {
        |s, span| self._pattern(s, span)
    }

    fn _sequence_type(
        &self,
        s: &str,
        _span: Span,
    ) -> Result<xpath_ast::SequenceType, AttributeError> {
        Ok(parse_sequence_type(
            s,
            &self.content.parser_context().namespaces,
        )?)
    }

    pub(crate) fn sequence_type(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<xpath_ast::SequenceType, AttributeError> + '_ {
        |s, span| self._sequence_type(s, span)
    }

    fn _item_type(&self, s: &str, _span: Span) -> Result<xpath_ast::ItemType, AttributeError> {
        Ok(parse_item_type(
            s,
            &self.content.parser_context().namespaces,
        )?)
    }

    pub(crate) fn item_type(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<xpath_ast::ItemType, AttributeError> + '_ {
        |s, span| self._item_type(s, span)
    }

    fn _decimal(s: &str, span: Span) -> Result<Decimal, AttributeError> {
        let s = Self::trim_token(s);
        Decimal::from_str(s).map_err(|_| AttributeError::Invalid {
            value: s.to_string(),
            span,
        })
    }

    pub(crate) fn decimal(&self) -> impl Fn(&'a str, Span) -> Result<Decimal, AttributeError> + '_ {
        Self::_decimal
    }

    fn _method(&self, s: &str, span: Span) -> Result<ast::OutputMethod, AttributeError> {
        use ast::OutputMethod::*;

        match s {
            "xml" => Ok(Xml),
            "html" => Ok(Html),
            "xhtml" => Ok(Xhtml),
            "text" => Ok(Text),
            "json" => Ok(Json),
            "adaptive" => Ok(Adaptive),
            _ => Ok(EqName(self._eqname(s, span)?)),
        }
    }

    pub(crate) fn method(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::OutputMethod, AttributeError> + '_ {
        |s, span| self._method(s, span)
    }

    fn _json_node_output_method(
        &self,
        s: &str,
        span: Span,
    ) -> Result<ast::JsonNodeOutputMethod, AttributeError> {
        use ast::JsonNodeOutputMethod::*;

        match s {
            "xml" => Ok(Xml),
            "html" => Ok(Html),
            "xhtml" => Ok(Xhtml),
            "text" => Ok(Text),
            _ => Ok(EqName(self._eqname(s, span)?)),
        }
    }

    pub(crate) fn json_node_output_method(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::JsonNodeOutputMethod, AttributeError> + '_ {
        |s, span| self._json_node_output_method(s, span)
    }

    fn _data_type(&self, s: &str, span: Span) -> Result<ast::DataType, AttributeError> {
        use ast::DataType::*;

        match s {
            "text" => Ok(Text),
            "number" => Ok(Number),
            _ => Ok(EQName(self._eqname(s, span)?)),
        }
    }

    pub(crate) fn data_type(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::DataType, AttributeError> + '_ {
        |s, span| self._data_type(s, span)
    }

    fn _streamability(&self, s: &str, span: Span) -> Result<ast::Streamability, AttributeError> {
        use ast::Streamability::*;

        match s {
            "unclassified" => Ok(Unclassified),
            "absorbing" => Ok(Absorbing),
            "inspection" => Ok(Inspection),
            "filter" => Ok(Filter),
            "shallow-descent" => Ok(ShallowDescent),
            "deep-descent" => Ok(DeepDescent),
            "ascent" => Ok(Ascent),
            _ => Ok(EqName(self._eqname(s, span)?)),
        }
    }

    pub(crate) fn streamability(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::Streamability, AttributeError> + '_ {
        |s, span| self._streamability(s, span)
    }

    fn _input_type_annotations(
        s: &str,
        span: Span,
    ) -> Result<ast::InputTypeAnnotations, AttributeError> {
        match s {
            "strip" => Ok(ast::InputTypeAnnotations::Strip),
            "preserve" => Ok(ast::InputTypeAnnotations::Preserve),
            "unspecified" => Ok(ast::InputTypeAnnotations::Unspecified),
            _ => Err(AttributeError::Invalid {
                value: s.to_string(),
                span,
            }),
        }
    }

    pub(crate) fn input_type_annotations(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::InputTypeAnnotations, AttributeError> + '_ {
        Self::_input_type_annotations
    }

    fn _phase(s: &str, span: Span) -> Result<ast::AccumulatorPhase, AttributeError> {
        match s {
            "start" => Ok(ast::AccumulatorPhase::Start),
            "end" => Ok(ast::AccumulatorPhase::End),
            _ => Err(AttributeError::Invalid {
                value: s.to_string(),
                span,
            }),
        }
    }

    pub(crate) fn phase(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::AccumulatorPhase, AttributeError> + '_ {
        Self::_phase
    }

    fn _component(s: &str, span: Span) -> Result<ast::Component, AttributeError> {
        use ast::Component::*;

        match s {
            "template" => Ok(Template),
            "function" => Ok(Function),
            "attribute-set" => Ok(AttributeSet),
            "variable" => Ok(Variable),
            "mode" => Ok(Mode),
            "*" => Ok(Star),
            _ => Err(AttributeError::Invalid {
                value: s.to_string(),
                span,
            }),
        }
    }

    pub(crate) fn component(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::Component, AttributeError> + '_ {
        Self::_component
    }

    fn _visibility(s: &str, span: Span) -> Result<ast::Visibility, AttributeError> {
        use ast::Visibility::*;

        match s {
            "public" => Ok(Public),
            "private" => Ok(Private),
            "final" => Ok(Final),
            _ => Err(AttributeError::Invalid {
                value: s.to_string(),
                span,
            }),
        }
    }

    pub(crate) fn visibility(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::Visibility, AttributeError> + '_ {
        Self::_visibility
    }

    fn _visibility_with_abstract(
        s: &str,
        span: Span,
    ) -> Result<ast::VisibilityWithAbstract, AttributeError> {
        use ast::VisibilityWithAbstract::*;

        match s {
            "public" => Ok(Public),
            "private" => Ok(Private),
            "final" => Ok(Final),
            "abstract" => Ok(Abstract),
            _ => Err(AttributeError::Invalid {
                value: s.to_string(),
                span,
            }),
        }
    }

    pub(crate) fn visibility_with_abstract(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::VisibilityWithAbstract, AttributeError> {
        Self::_visibility_with_abstract
    }

    fn _visibility_with_hidden(
        s: &str,
        span: Span,
    ) -> Result<ast::VisibilityWithHidden, AttributeError> {
        use ast::VisibilityWithHidden::*;

        match s {
            "public" => Ok(Public),
            "private" => Ok(Private),
            "final" => Ok(Final),
            "hidden" => Ok(Hidden),
            _ => Err(AttributeError::Invalid {
                value: s.to_string(),
                span,
            }),
        }
    }

    pub(crate) fn visibility_with_hidden(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::VisibilityWithHidden, AttributeError> {
        Self::_visibility_with_hidden
    }

    fn _validation(s: &str, span: Span) -> Result<ast::Validation, AttributeError> {
        use ast::Validation::*;

        match s {
            "strict" => Ok(Strict),
            "lax" => Ok(Lax),
            "preserve" => Ok(Preserve),
            "strip" => Ok(Strip),
            _ => Err(AttributeError::Invalid {
                value: s.to_string(),
                span,
            }),
        }
    }

    pub(crate) fn validation(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::Validation, AttributeError> + '_ {
        Self::_validation
    }

    fn _on_no_match(s: &str, span: Span) -> Result<ast::OnNoMatch, AttributeError> {
        let s = Self::trim_token(s);
        use ast::OnNoMatch::*;

        match s {
            "deep-copy" => Ok(DeepCopy),
            "shallow-copy" => Ok(ShallowCopy),
            "deep-skip" => Ok(DeepSkip),
            "shallow-skip" => Ok(ShallowSkip),
            "text-only-copy" => Ok(TextOnlyCopy),
            "fail" => Ok(Fail),
            _ => Err(AttributeError::Invalid {
                value: s.to_string(),
                span,
            }),
        }
    }

    pub(crate) fn on_no_match(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::OnNoMatch, AttributeError> + '_ {
        Self::_on_no_match
    }

    fn _on_multiple_match(s: &str, span: Span) -> Result<ast::OnMultipleMatch, AttributeError> {
        let s = Self::trim_token(s);
        use ast::OnMultipleMatch::*;

        match s {
            "use-last" => Ok(UseLast),
            "fail" => Ok(Fail),
            _ => Err(AttributeError::Invalid {
                value: s.to_string(),
                span,
            }),
        }
    }

    pub(crate) fn on_multiple_match(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::OnMultipleMatch, AttributeError> + '_ {
        Self::_on_multiple_match
    }

    fn _typed(s: &str, span: Span) -> Result<ast::Typed, AttributeError> {
        let s = Self::trim_token(s);
        use ast::Typed::*;

        match s {
            "yes" => Ok(Yes),
            "no" => Ok(No),
            "strict" => Ok(Strict),
            "lax" => Ok(Lax),
            "unspecified" => Ok(Unspecified),
            _ => Err(AttributeError::Invalid {
                value: s.to_string(),
                span,
            }),
        }
    }

    pub(crate) fn typed(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::Typed, AttributeError> + '_ {
        Self::_typed
    }

    fn _level(s: &str, span: Span) -> Result<ast::NumberLevel, AttributeError> {
        use ast::NumberLevel::*;

        match s {
            "single" => Ok(Single),
            "multiple" => Ok(Multiple),
            "any" => Ok(Any),
            _ => Err(AttributeError::Invalid {
                value: s.to_string(),
                span,
            }),
        }
    }

    pub(crate) fn level(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::NumberLevel, AttributeError> + '_ {
        Self::_level
    }

    fn _letter_value(s: &str, span: Span) -> Result<ast::LetterValue, AttributeError> {
        use ast::LetterValue::*;

        match s {
            "alphabetic" => Ok(Alphabetic),
            "traditional" => Ok(Traditional),
            _ => Err(AttributeError::Invalid {
                value: s.to_string(),
                span,
            }),
        }
    }

    pub(crate) fn letter_value(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::LetterValue, AttributeError> + '_ {
        Self::_letter_value
    }

    fn _normalization_form(
        &self,
        s: &str,
        span: Span,
    ) -> Result<ast::NormalizationForm, AttributeError> {
        use ast::NormalizationForm::*;

        match s {
            "NFC" => Ok(Nfc),
            "NFD" => Ok(Nfd),
            "NFKC" => Ok(Nfkc),
            "NFKD" => Ok(Nfkd),
            _ => Ok(NmToken(Self::_nmtoken(s, span)?)),
        }
    }

    pub(crate) fn normalization_form(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::NormalizationForm, AttributeError> + '_ {
        |s, span| self._normalization_form(s, span)
    }

    fn _standalone(s: &str, _span: Span) -> Result<ast::Standalone, AttributeError> {
        let s = s.trim();
        match s {
            "yes" | "1" | "true" => Ok(ast::Standalone::Bool(true)),
            "no" | "0" | "false" => Ok(ast::Standalone::Bool(false)),
            "omit" => Ok(ast::Standalone::Omit),
            _ => Err(AttributeError::Invalid {
                value: s.to_string(),
                span: _span,
            }),
        }
    }

    pub(crate) fn standalone(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::Standalone, AttributeError> + '_ {
        Self::_standalone
    }

    fn _language(s: &str, _span: Span) -> Result<ast::Language, AttributeError> {
        // TODO
        Ok(s.to_string())
    }

    pub(crate) fn language(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::Language, AttributeError> + '_ {
        Self::_language
    }

    fn _order(s: &str, span: Span) -> Result<ast::Order, AttributeError> {
        use ast::Order::*;

        match s {
            "ascending" => Ok(Ascending),
            "descending" => Ok(Descending),
            _ => Err(AttributeError::Invalid {
                value: s.to_string(),
                span,
            }),
        }
    }

    pub(crate) fn order(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::Order, AttributeError> + '_ {
        Self::_order
    }

    fn _case_order(s: &str, span: Span) -> Result<ast::CaseOrder, AttributeError> {
        use ast::CaseOrder::*;

        match s {
            "upper-first" => Ok(UpperFirst),
            "lower-first" => Ok(LowerFirst),
            _ => Err(AttributeError::Invalid {
                value: s.to_string(),
                span,
            }),
        }
    }

    pub(crate) fn case_order(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::CaseOrder, AttributeError> + '_ {
        Self::_case_order
    }
    fn _use(s: &str, span: Span) -> Result<ast::Use, AttributeError> {
        use ast::Use::*;

        match s {
            "optional" => Ok(Optional),
            "required" => Ok(Required),
            "absent" => Ok(Absent),
            _ => Err(AttributeError::Invalid {
                value: s.to_string(),
                span,
            }),
        }
    }

    pub(crate) fn use_(&self) -> impl Fn(&'a str, Span) -> Result<ast::Use, AttributeError> {
        Self::_use
    }

    fn _new_each_time(s: &str, span: Span) -> Result<ast::NewEachTime, AttributeError> {
        use ast::NewEachTime::*;

        let s = s.trim();

        match s {
            "yes" | "1" | "true" => Ok(Yes),
            "no" | "0" | "false" => Ok(No),
            "maybe" => Ok(Maybe),
            _ => Err(AttributeError::Invalid {
                value: s.to_string(),
                span,
            }),
        }
    }

    pub(crate) fn new_each_time(
        &self,
    ) -> impl Fn(&'a str, Span) -> Result<ast::NewEachTime, AttributeError> + '_ {
        Self::_new_each_time
    }
}

fn rewrite_large_decimal_format_number_call(s: &str) -> Option<String> {
    let mut result = String::new();
    let mut cursor = 0;
    let mut index = 0;
    let mut changed = false;

    while index < s.len() {
        let Some(ch) = s[index..].chars().next() else {
            break;
        };
        if ch == '\'' || ch == '"' {
            index = skip_xpath_string(s, index, ch);
            continue;
        }

        if let Some(rewrite) = parse_format_number_rewrite(s, index) {
            result.push_str(&s[cursor..rewrite.name_start]);
            result.push_str("format-number-lexical");
            result.push_str(&s[rewrite.name_end..rewrite.arg_start]);
            result.push('\'');
            result.push_str(&rewrite.lexical);
            result.push('\'');
            cursor = rewrite.arg_end;
            index = rewrite.call_end;
            changed = true;
            continue;
        }

        index += ch.len_utf8();
    }

    if !changed {
        return None;
    }
    result.push_str(&s[cursor..]);
    Some(result)
}

#[derive(Debug)]
struct FormatNumberRewrite {
    name_start: usize,
    name_end: usize,
    arg_start: usize,
    arg_end: usize,
    call_end: usize,
    lexical: String,
}

fn parse_format_number_rewrite(s: &str, index: usize) -> Option<FormatNumberRewrite> {
    const NAME: &str = "format-number";

    if !s[index..].starts_with(NAME) {
        return None;
    }

    if index > 0 {
        let previous = s[..index].chars().next_back()?;
        if is_xpath_name_char(previous) {
            return None;
        }
    }

    let name_end = index + NAME.len();
    let mut open_paren = name_end;
    while let Some(ch) = s[open_paren..].chars().next() {
        if !ch.is_whitespace() {
            break;
        }
        open_paren += ch.len_utf8();
    }
    if !s[open_paren..].starts_with('(') {
        return None;
    }

    let mut depth_paren = 1usize;
    let mut depth_bracket = 0usize;
    let mut depth_brace = 0usize;
    let mut comma = None;
    let mut position = open_paren + 1;

    while position < s.len() {
        let ch = s[position..].chars().next()?;
        if ch == '\'' || ch == '"' {
            position = skip_xpath_string(s, position, ch);
            continue;
        }
        match ch {
            '(' => depth_paren += 1,
            ')' => {
                depth_paren -= 1;
                if depth_paren == 0 {
                    let comma = comma?;
                    let raw_first_arg = &s[open_paren + 1..comma];
                    let lexical = raw_first_arg.trim();
                    if !is_large_decimal_literal(lexical) {
                        return None;
                    }
                    return Some(FormatNumberRewrite {
                        name_start: index,
                        name_end,
                        arg_start: open_paren + 1,
                        arg_end: comma,
                        call_end: position + ch.len_utf8(),
                        lexical: lexical.to_string(),
                    });
                }
            }
            '[' => depth_bracket += 1,
            ']' => depth_bracket = depth_bracket.saturating_sub(1),
            '{' => depth_brace += 1,
            '}' => depth_brace = depth_brace.saturating_sub(1),
            ',' if depth_paren == 1
                && depth_bracket == 0
                && depth_brace == 0
                && comma.is_none() =>
            {
                comma = Some(position);
            }
            _ => {}
        }
        position += ch.len_utf8();
    }

    None
}

fn is_large_decimal_literal(s: &str) -> bool {
    let lexical = s.strip_prefix(['+', '-']).unwrap_or(s);
    if lexical.parse::<Decimal>().is_ok() {
        return false;
    }

    let mut seen_dot = false;
    let mut digit_count = 0usize;
    for ch in lexical.chars() {
        match ch {
            '.' if !seen_dot => seen_dot = true,
            '0'..='9' => digit_count += 1,
            _ => return false,
        }
    }
    seen_dot && digit_count > 0
}

fn skip_xpath_string(s: &str, start: usize, quote: char) -> usize {
    let mut position = start + quote.len_utf8();
    while position < s.len() {
        let ch = match s[position..].chars().next() {
            Some(ch) => ch,
            None => break,
        };
        position += ch.len_utf8();
        if ch != quote {
            continue;
        }
        if s[position..].starts_with(quote) {
            position += quote.len_utf8();
            continue;
        }
        break;
    }
    position
}

fn is_xpath_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_large_decimal_literal_in_format_number_call() {
        let source =
            "format-number(000123456789012345678901234567890.123456789012345678900000, '##0.0')";
        let rewritten = rewrite_large_decimal_format_number_call(source).unwrap();
        assert_eq!(
            rewritten,
            "format-number-lexical('000123456789012345678901234567890.123456789012345678900000', '##0.0')"
        );
    }

    #[test]
    fn does_not_rewrite_regular_decimal_literal() {
        let source = "format-number(12.34, '##0.0')";
        assert!(rewrite_large_decimal_format_number_call(source).is_none());
    }
}
