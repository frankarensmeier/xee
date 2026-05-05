//! Top-level XSLT declaration compilation.
//!
//! Compiles stylesheet-level declarations into IR: templates (named and match),
//! keys, modes, output parameters, accumulators, global variables/params,
//! user-defined functions, attribute sets, character maps, and namespace aliases.

use rust_decimal::Decimal;
use xee_name::Name;

use std::collections::HashSet;
use xee_interpreter::{
    error,
    interpreter::instruction::RaisedError,
    sequence::QNameOrString,
};
use xee_ir::{ir, Bindings};
use xee_xpath_ast::{ast as xpath_ast, pattern::transform_pattern, span::Spanned};
use xee_xslt_ast::ast;
use xot::{
    xmlname::{NameStrInfo, OwnedName},
    Xot,
};

use super::{
    adjusted_span, validate_non_reserved_stylesheet_name,
    IrConverter, PreprocessedDeclaration, LoadedOutputParameterDocument,
    SERIALIZATION_NAMESPACE,
};
use crate::priority::default_priority;

impl<'a> IrConverter<'a> {
    /// Main entry point: compile all preprocessed declarations into IR.
    ///
    /// This runs in two phases: first it collects stylesheet-level metadata
    /// (attribute sets, character maps, namespace aliases, global variables, etc.),
    /// then it iterates declarations to compile templates, keys, modes, and output.
    pub(super) fn transform(
        &mut self,
        declarations: &[PreprocessedDeclaration],
    ) -> error::SpannedResult<ir::Declarations> {
        self.strip_source_document_whitespace =
            self.should_strip_source_document_whitespace(declarations);
        self.validate_reserved_declaration_names(declarations)?;
        self.register_xslt_function_names(declarations)?;
        self.collect_attribute_sets(declarations);
        self.validate_attribute_set_references()?;
        self.collect_named_outputs(declarations);
        self.collect_character_maps(declarations);
        self.collect_named_templates_with_absent_context(declarations);
        self.collect_namespace_aliases(declarations);
        self.collect_accumulators(declarations);
        // Register global variable/param names early so $var references resolve.
        let global_vars = self.collect_global_variables(declarations)?;

        let main_sequence_constructor = self.main_sequence_constructor();
        let main = self.sequence_constructor_function(&main_sequence_constructor)?;
        let mut ir_declarations = ir::Declarations::new(main);
        ir_declarations.global_variables = global_vars;
        ir_declarations.strip_space_all = self.strip_source_document_whitespace;

        for declaration in declarations {
            self.with_declaration_base_uri(declaration, |this| {
                this.declaration(&mut ir_declarations, declaration)
            })?;
        }

        self.compile_referenced_accumulators(&mut ir_declarations)?;

        // Move accumulated number patterns into IR declarations
        ir_declarations.number_patterns = std::mem::take(&mut self.number_patterns);

        Ok(ir_declarations)
    }

    pub(super) fn should_strip_source_document_whitespace(
        &self,
        declarations: &[PreprocessedDeclaration],
    ) -> bool {
        let strip_all = declarations.iter().any(|declaration| {
            let ast::Declaration::StripSpace(strip_space) = &declaration.declaration else {
                return false;
            };

            strip_space.elements.iter().any(|element| element == "*")
        });

        strip_all
            && !declarations.iter().any(|declaration| {
                matches!(declaration.declaration, ast::Declaration::PreserveSpace(_))
            })
    }

    pub(super) fn validate_reserved_declaration_names(
        &self,
        declarations: &[PreprocessedDeclaration],
    ) -> error::SpannedResult<()> {
        for declaration in declarations {
            match &declaration.declaration {
                ast::Declaration::Accumulator(accumulator) => {
                    validate_non_reserved_stylesheet_name(&accumulator.name, accumulator.span)?;
                }
                ast::Declaration::AttributeSet(attribute_set) => {
                    validate_non_reserved_stylesheet_name(&attribute_set.name, attribute_set.span)?;
                }
                ast::Declaration::CharacterMap(character_map) => {
                    validate_non_reserved_stylesheet_name(&character_map.name, character_map.span)?;
                }
                ast::Declaration::DecimalFormat(decimal_format) => {
                    if let Some(name) = &decimal_format.name {
                        validate_non_reserved_stylesheet_name(name, decimal_format.span)?;
                    }
                }
                ast::Declaration::Function(function) => {
                    validate_non_reserved_stylesheet_name(&function.name, function.span)?;
                }
                ast::Declaration::Key(key) => {
                    validate_non_reserved_stylesheet_name(&key.name, key.span)?;
                }
                ast::Declaration::Mode(mode) => {
                    if let Some(name) = &mode.name {
                        validate_non_reserved_stylesheet_name(name, mode.span)?;
                    }
                }
                ast::Declaration::Output(output) => {
                    if let Some(name) = &output.name {
                        validate_non_reserved_stylesheet_name(name, output.span)?;
                    }
                }
                ast::Declaration::Param(param) => {
                    validate_non_reserved_stylesheet_name(&param.name, param.span)?;
                }
                ast::Declaration::Template(template) => {
                    if let Some(name) = &template.name {
                        validate_non_reserved_stylesheet_name(name, template.span)?;
                    }
                }
                ast::Declaration::Variable(variable) => {
                    validate_non_reserved_stylesheet_name(&variable.name, variable.span)?;
                }
                _ => {}
            }
        }

        Ok(())
    }

    pub(super) fn collect_namespace_aliases(&mut self, declarations: &[PreprocessedDeclaration]) {
        for declaration in declarations {
            let ast::Declaration::NamespaceAlias(namespace_alias) = &declaration.declaration else {
                continue;
            };
            self.namespace_aliases.insert(
                namespace_alias.stylesheet_namespace.clone(),
                namespace_alias.result_namespace.clone(),
            );
        }
    }

    pub(super) fn attribute_set_key(name: &ast::EqName) -> (String, String) {
        (name.namespace().to_string(), name.local_name().to_string())
    }

    pub(super) fn character_map_key(name: &ast::EqName) -> (String, String) {
        (name.namespace().to_string(), name.local_name().to_string())
    }

    pub(super) fn output_key(name: &ast::EqName) -> (String, String) {
        (name.namespace().to_string(), name.local_name().to_string())
    }

    pub(super) fn collect_attribute_sets(&mut self, declarations: &[PreprocessedDeclaration]) {
        for declaration in declarations {
            let ast::Declaration::AttributeSet(attribute_set) = &declaration.declaration else {
                continue;
            };
            self.attribute_sets
                .entry(Self::attribute_set_key(&attribute_set.name))
                .or_default()
                .push((**attribute_set).clone());
        }
    }

    pub(super) fn validate_attribute_set_references(&self) -> error::SpannedResult<()> {
        for key in self.attribute_sets.keys() {
            self.validate_attribute_set_references_for(key, &mut HashSet::new())?;
        }
        Ok(())
    }

    pub(super) fn validate_attribute_set_references_for(
        &self,
        key: &(String, String),
        visiting: &mut HashSet<(String, String)>,
    ) -> error::SpannedResult<()> {
        if !visiting.insert(key.clone()) {
            return Ok(());
        }

        let Some(attribute_sets) = self.attribute_sets.get(key) else {
            return Err(error::Error::XTSE0710.into());
        };

        for attribute_set in attribute_sets {
            if let Some(use_attribute_sets) = &attribute_set.use_attribute_sets {
                for name in use_attribute_sets {
                    let nested_key = Self::attribute_set_key(name);
                    if !self.attribute_sets.contains_key(&nested_key) {
                        return Err(error::Error::XTSE0710.into());
                    }
                    self.validate_attribute_set_references_for(&nested_key, visiting)?;
                }
            }
        }

        visiting.remove(key);
        Ok(())
    }

    pub(super) fn collect_character_maps(&mut self, declarations: &[PreprocessedDeclaration]) {
        for declaration in declarations {
            let ast::Declaration::CharacterMap(character_map) = &declaration.declaration else {
                continue;
            };
            let key = Self::character_map_key(&character_map.name);
            let should_replace = match self.character_maps.get(&key) {
                Some((precedence, _)) => declaration.import_precedence >= *precedence,
                None => true,
            };
            if should_replace {
                self.character_maps.insert(
                    key,
                    (declaration.import_precedence, (**character_map).clone()),
                );
            }
        }
    }

    pub(super) fn collect_named_outputs(&mut self, declarations: &[PreprocessedDeclaration]) {
        for declaration in declarations {
            let ast::Declaration::Output(output) = &declaration.declaration else {
                continue;
            };
            let Some(name) = &output.name else {
                continue;
            };
            let key = Self::output_key(name);
            let should_replace = match self.named_outputs.get(&key) {
                Some((precedence, _)) => declaration.import_precedence >= *precedence,
                None => true,
            };
            if should_replace {
                self.named_outputs
                    .insert(key, (declaration.import_precedence, (**output).clone()));
            }
        }
    }

    pub(super) fn resolve_character_maps(
        &mut self,
        names: &[ast::EqName],
    ) -> error::SpannedResult<ahash::HashMap<char, String>> {
        let mut resolved = ahash::HashMap::default();
        let mut visiting = HashSet::new();
        for name in names {
            for (character, replacement) in self.resolve_character_map(name, &mut visiting)? {
                resolved.insert(character, replacement);
            }
        }
        Ok(resolved)
    }

    pub(super) fn resolve_character_map(
        &mut self,
        name: &ast::EqName,
        visiting: &mut HashSet<(String, String)>,
    ) -> error::SpannedResult<ahash::HashMap<char, String>> {
        let key = Self::character_map_key(name);
        if let Some(resolved) = self.resolved_character_maps.get(&key) {
            return Ok(resolved.clone());
        }
        if !visiting.insert(key.clone()) {
            return Err(error::Error::Unsupported(
                "Circular xsl:character-map dependencies are not supported yet".to_string(),
            )
            .into());
        }

        let Some((_, character_map)) = self.character_maps.get(&key).cloned() else {
            return Err(error::Error::Unsupported(format!(
                "Unknown xsl:character-map {}",
                name.local_name()
            ))
            .into());
        };

        let mut resolved = ahash::HashMap::default();
        if let Some(used_character_maps) = &character_map.use_character_maps {
            for used_character_map in used_character_maps {
                for (character, replacement) in
                    self.resolve_character_map(used_character_map, visiting)?
                {
                    resolved.insert(character, replacement);
                }
            }
        }
        for output_character in character_map.output_characters {
            resolved.insert(output_character.character, output_character.string);
        }

        visiting.remove(&key);
        self.resolved_character_maps
            .insert(key.clone(), resolved.clone());
        Ok(resolved)
    }

    pub(super) fn encode_character_maps(character_maps: &ahash::HashMap<char, String>) -> String {
        let mut entries = character_maps.iter().collect::<Vec<_>>();
        entries.sort_by_key(|(character, _)| **character as u32);
        entries
            .into_iter()
            .map(|(character, replacement)| {
                format!(
                    "{:x}={}",
                    *character as u32,
                    replacement
                        .as_bytes()
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>()
                )
            })
            .collect::<Vec<_>>()
            .join("|")
    }

    pub(super) fn encode_named_outputs(&mut self) -> error::SpannedResult<String> {
        let mut outputs = self
            .named_outputs
            .iter()
            .map(|((namespace, local_name), (_, output))| {
                (namespace.clone(), local_name.clone(), output.clone())
            })
            .collect::<Vec<_>>();
        outputs.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));

        outputs
            .into_iter()
            .map(|(namespace, local_name, output)| {
                let fields = vec![
                    namespace,
                    local_name,
                    Self::output_method_literal(output.method.as_ref())?.unwrap_or_default(),
                    output.byte_order_mark.to_string(),
                    Self::qname_list_literal(&output.cdata_section_elements),
                    output.doctype_public.unwrap_or_default(),
                    output.doctype_system.unwrap_or_default(),
                    output.include_content_type.to_string(),
                    output.media_type.unwrap_or_default(),
                    output.item_separator.unwrap_or_default(),
                    output.omit_xml_declaration.map(|b| b.to_string()).unwrap_or_default(),
                    Self::output_standalone_literal(output.standalone.as_ref()).unwrap_or_default(),
                    output
                        .html_version
                        .map(|html_version| html_version.to_string())
                        .unwrap_or_default(),
                    Self::encode_character_maps(&self.resolve_character_maps(&output.use_character_maps)?),
                    output.version.unwrap_or_default(),
                ];
                Ok(fields
                    .into_iter()
                    .map(|field| Self::hex_encode(&field))
                    .collect::<Vec<_>>()
                    .join(","))
            })
            .collect::<error::SpannedResult<Vec<_>>>()
            .map(|entries| entries.join(";"))
    }

    pub(super) fn resolve_named_output(
        &self,
        format: &ast::ValueTemplate<ast::EqName>,
        namespaces: &[ast::LiteralNamespace],
    ) -> error::SpannedResult<ast::Output> {
        let lexical_qname = self.static_value_template(format).ok_or_else(|| {
            error::Error::Unsupported(
                "Dynamic xsl:result-document @format is not supported yet".to_string(),
            )
        })?;
        let key = if let Some((local_name, namespace)) =
            self.resolve_static_qname_with_default(&lexical_qname, namespaces, "")
        {
            (namespace, local_name)
        } else {
            (String::new(), lexical_qname)
        };
        let Some((_, output)) = self.named_outputs.get(&key) else {
            return Err(error::Error::Unsupported(
                "Unknown xsl:result-document @format".to_string(),
            )
            .into());
        };
        Ok(output.clone())
    }

    pub(super) fn output_method_literal(
        method: Option<&ast::OutputMethod>,
    ) -> error::SpannedResult<Option<String>> {
        Ok(match method {
            Some(ast::OutputMethod::Adaptive) => Some("adaptive".to_string()),
            Some(ast::OutputMethod::Xml) => Some("xml".to_string()),
            Some(ast::OutputMethod::Html) => Some("html".to_string()),
            Some(ast::OutputMethod::Xhtml) => Some("xhtml".to_string()),
            Some(ast::OutputMethod::Text) => Some("text".to_string()),
            Some(ast::OutputMethod::Json) => Some("json".to_string()),
            None => None,
            method => {
                return Err(error::Error::Unsupported(format!(
                    "Output method {:?} not supported yet",
                    method
                ))
                .into())
            }
        })
    }

    pub(super) fn output_standalone_literal(standalone: Option<&ast::Standalone>) -> Option<String> {
        match standalone {
            Some(ast::Standalone::Bool(true)) => Some("yes".to_string()),
            Some(ast::Standalone::Bool(false)) => Some("no".to_string()),
            Some(ast::Standalone::Omit) => Some("omit".to_string()),
            None => None,
        }
    }

    pub(super) fn validate_standalone_literal(value: &str) -> error::SpannedResult<String> {
        let trimmed = value.trim();
        match trimmed {
            "yes" | "true" | "1" | "no" | "false" | "0" | "omit" => Ok(trimmed.to_string()),
            _ => Err(error::Error::XTSE0020.into()),
        }
    }

    pub(super) fn validate_boolean_literal(value: &str) -> error::SpannedResult<String> {
        let trimmed = value.trim();
        match trimmed {
            "yes" | "true" | "1" | "no" | "false" | "0" => Ok(trimmed.to_string()),
            _ => Err(error::Error::XTSE0020.into()),
        }
    }

    pub(super) fn value_template_or_literal_atom<V>(
        &mut self,
        value_template: Option<&ast::ValueTemplate<V>>,
        default: String,
    ) -> error::SpannedResult<(ir::AtomS, Bindings)>
    where
        V: Clone + PartialEq + Eq,
    {
        Ok(if let Some(value_template) = value_template {
            if let Some(literal) = self.static_value_template(value_template) {
                (
                    Spanned::new(ir::Atom::Const(ir::Const::String(literal)), (0..0).into()),
                    Bindings::empty(),
                )
            } else {
                self.attribute_value_template(value_template)?.atom_bindings()
            }
        } else {
            (
                Spanned::new(ir::Atom::Const(ir::Const::String(default)), (0..0).into()),
                Bindings::empty(),
            )
        })
    }

    pub(super) fn validated_value_template_or_literal_atom<V>(
        &mut self,
        value_template: Option<&ast::ValueTemplate<V>>,
        default: String,
        validator: fn(&str) -> error::SpannedResult<String>,
    ) -> error::SpannedResult<(ir::AtomS, Bindings)>
    where
        V: Clone + PartialEq + Eq,
    {
        Ok(if let Some(value_template) = value_template {
            if let Some(literal) = self.static_value_template(value_template) {
                (
                    Spanned::new(
                        ir::Atom::Const(ir::Const::String(validator(&literal)?)),
                        (0..0).into(),
                    ),
                    Bindings::empty(),
                )
            } else {
                self.attribute_value_template(value_template)?.atom_bindings()
            }
        } else {
            (
                Spanned::new(ir::Atom::Const(ir::Const::String(default)), (0..0).into()),
                Bindings::empty(),
            )
        })
    }

    pub(super) fn qname_list_literal(names: &[ast::EqName]) -> String {
        names
            .iter()
            .map(|name| {
                if name.prefix().is_empty() {
                    name.local_name().to_string()
                } else {
                    format!("{}:{}", name.prefix(), name.local_name())
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub(super) fn merge_literal_qname_lists(base: &[ast::EqName], extra: Option<String>) -> String {
        let mut merged = base
            .iter()
            .map(|name| {
                if name.prefix().is_empty() {
                    name.local_name().to_string()
                } else {
                    format!("{}:{}", name.prefix(), name.local_name())
                }
            })
            .collect::<Vec<_>>();
        if let Some(extra) = extra {
            for token in extra.split_ascii_whitespace() {
                if !merged.iter().any(|existing| existing == token) {
                    merged.push(token.to_string());
                }
            }
        }
        merged.join(" ")
    }

    pub(super) fn collect_named_templates_with_absent_context(
        &mut self,
        declarations: &[PreprocessedDeclaration],
    ) {
        for declaration in declarations {
            let ast::Declaration::Template(template) = &declaration.declaration else {
                continue;
            };
            let Some(name) = &template.name else {
                continue;
            };
            let has_absent_context = matches!(
                template
                    .context_item
                    .as_ref()
                    .and_then(|context_item| context_item.use_.as_ref()),
                Some(ast::Use::Absent)
            );
            if has_absent_context {
                self.named_templates_with_absent_context
                    .insert(name.local_name().to_string());
            }
        }
    }

    pub(super) fn collect_accumulators(&mut self, declarations: &[PreprocessedDeclaration]) {
        for declaration in declarations {
            let ast::Declaration::Accumulator(accumulator) = &declaration.declaration else {
                continue;
            };

            let should_replace = self
                .accumulator_declarations
                .get(&accumulator.name)
                .map(|existing| declaration.import_precedence >= existing.import_precedence)
                .unwrap_or(true);
            if should_replace {
                self.accumulator_declarations
                    .insert(accumulator.name.clone(), declaration.clone());
            }
        }
    }

    pub(super) fn apply_namespace_alias(&self, name: &ast::Name) -> ast::Name {
        let Some(namespace) = self.namespace_aliases.get(name.namespace()) else {
            return name.clone();
        };

        let prefix = if namespace.is_empty() {
            String::new()
        } else {
            name.prefix().to_string()
        };
        Name::new(name.local_name().to_string(), namespace.clone(), prefix)
    }

    pub(super) fn register_xslt_function_names(
        &mut self,
        declarations: &[PreprocessedDeclaration],
    ) -> error::SpannedResult<()> {
        for declaration in declarations {
            let ast::Declaration::Function(function) = &declaration.declaration else {
                continue;
            };

            let arity = u8::try_from(function.params.len()).map_err(|_| {
                error::Error::Unsupported("Too many XSLT function parameters".to_string())
            })?;

            let hidden_name = OwnedName::new(
                format!("function-{}", self.xslt_function_counter),
                "urn:xee:internal:function".to_string(),
                "xee-internal".to_string(),
            );
            self.xslt_function_counter += 1;
            self.xslt_functions
                .insert((function.name.clone(), arity), hidden_name.clone());
            self.variables.new_var_name(&hidden_name);
        }

        Ok(())
    }

    /// Dispatch a single top-level declaration to its specialized compiler method.
    pub(super) fn declaration(
        &mut self,
        declarations: &mut ir::Declarations,
        declaration: &PreprocessedDeclaration,
    ) -> error::SpannedResult<()> {
        use ast::Declaration::*;
        match &declaration.declaration {
            AttributeSet(_) => Ok(()),
            Template(template) => self.template(
                declarations,
                template,
                declaration.import_precedence,
                &declaration.module_path,
            ),
            Mode(mode) => self.mode(declarations, mode),
            Output(output) => self.output(declarations, output),
            // Import/Include already handled during pre-processing in parse_with_base_dir
            Import(_) | Include(_) => Ok(()),
            Key(key) => self.key(declarations, key),
            // These declarations are parsed but not yet compiled - skip gracefully
            // to allow stylesheets containing them to still process templates
            Function(_) | Variable(_) | Param(_) | StripSpace(_) | PreserveSpace(_)
            | DecimalFormat(_) | CharacterMap(_) | NamespaceAlias(_) | ImportSchema(_)
            | UsePackage(_) | GlobalContextItem(_) | Accumulator(_) => Ok(()),
        }
    }

    /// Compile accumulators that were actually referenced during compilation.
    /// Accumulators are demand-driven: we only compile those whose names
    /// appeared in accumulator-before() or accumulator-after() calls.
    pub(super) fn compile_referenced_accumulators(
        &mut self,
        declarations: &mut ir::Declarations,
    ) -> error::SpannedResult<()> {
        let mut compiled = HashSet::new();

        loop {
            let pending = self
                .referenced_accumulators
                .iter()
                .filter(|name| !compiled.contains(*name))
                .cloned()
                .collect::<Vec<_>>();
            if pending.is_empty() {
                break;
            }

            for name in pending {
                compiled.insert(name.clone());

                let Some(declaration) = self.accumulator_declarations.get(&name).cloned() else {
                    continue;
                };
                let ast::Declaration::Accumulator(accumulator) = &declaration.declaration else {
                    continue;
                };

                let accumulator_definition = self.with_declaration_base_uri(&declaration, |this| {
                    this.accumulator(accumulator)
                })?;
                declarations.accumulators.push(accumulator_definition);
            }
        }

        Ok(())
    }

    pub(super) fn collect_global_variables(
        &mut self,
        declarations: &[PreprocessedDeclaration],
    ) -> error::SpannedResult<Vec<ir::GlobalVariable>> {
        for decl in declarations {
            match &decl.declaration {
                ast::Declaration::Variable(var) => {
                    self.variables.new_var_name(&var.name);
                }
                ast::Declaration::Param(param) => {
                    self.variables.new_var_name(&param.name);
                }
                ast::Declaration::Function(function) => {
                    let arity = u8::try_from(function.params.len()).map_err(|_| {
                        error::Error::Unsupported("Too many XSLT function parameters".to_string())
                    })?;
                    let hidden_name = self
                        .xslt_functions
                        .get(&(function.name.clone(), arity))
                        .ok_or_else(|| {
                            error::Error::Unsupported("Unregistered XSLT function name".to_string())
                        })?;
                    self.variables.new_var_name(hidden_name);
                }
                _ => {}
            }
        }

        let mut globals = Vec::new();
        for decl in declarations {
            match &decl.declaration {
                ast::Declaration::Variable(var) => {
                    self.validate_variable(var)?;
                    let name = self.variables.lookup_var_name(&var.name).unwrap();
                    let expr = self.with_declaration_base_uri(decl, |this| {
                        this.with_hidden_global_name(&var.name, |this| {
                            let context_names = this.variables.push_context();
                            let params = Self::context_params(&context_names);
                            let expr = this.global_variable_expr(
                                var.select.as_ref(),
                                &var.sequence_constructor,
                                var.as_.as_ref(),
                                var.span,
                            )?;
                            this.variables.pop_context();
                            Ok((params, expr))
                        })
                    })?;
                    globals.push(ir::GlobalVariable {
                        name,
                        original_name: Some(var.name.clone()),
                        public_name: None,
                        public_arity: None,
                        external: false,
                        required: false,
                        params: expr.0,
                        expr: expr.1,
                        static_base_uri: decl.stylesheet_uri.clone(),
                    });
                }
                ast::Declaration::Param(param) => {
                    self.validate_param(param)?;
                    let name = self.variables.lookup_var_name(&param.name).unwrap();
                    let expr = self.with_declaration_base_uri(decl, |this| {
                        this.with_hidden_global_name(&param.name, |this| {
                            let context_names = this.variables.push_context();
                            let params = Self::context_params(&context_names);
                            let expr = this.global_param_expr(
                                param.select.as_ref(),
                                &param.sequence_constructor,
                            )?;
                            this.variables.pop_context();
                            Ok((params, expr))
                        })
                    })?;
                    globals.push(ir::GlobalVariable {
                        name,
                        original_name: Some(param.name.clone()),
                        public_name: None,
                        public_arity: None,
                        external: true,
                        required: param.required,
                        params: expr.0,
                        expr: expr.1,
                        static_base_uri: decl.stylesheet_uri.clone(),
                    });
                }
                ast::Declaration::Function(function) => {
                    let arity = u8::try_from(function.params.len()).map_err(|_| {
                        error::Error::Unsupported("Too many XSLT function parameters".to_string())
                    })?;
                    let hidden_name = self
                        .xslt_functions
                        .get(&(function.name.clone(), arity))
                        .ok_or_else(|| {
                            error::Error::Unsupported("Unregistered XSLT function name".to_string())
                        })?;
                    let name = self.variables.lookup_var_name(hidden_name).unwrap();
                    let (params, expr) = self.with_declaration_base_uri(decl, |this| {
                        let context_names = this.variables.push_context();
                        let params = Self::context_params(&context_names);
                        let function_definition = this.xslt_function_definition(function)?;
                        this.variables.pop_context();
                        let expr = Spanned::new(
                            ir::Expr::FunctionDefinition(function_definition),
                            adjusted_span(function.span, this.current_span_offset),
                        );
                        Ok((params, expr))
                    })?;
                    let (public_name, public_arity) = match function.visibility {
                        Some(ast::VisibilityWithAbstract::Public)
                        | Some(ast::VisibilityWithAbstract::Final) => {
                            (Some(function.name.clone()), Some(arity))
                        }
                        _ => (None, None),
                    };
                    globals.push(ir::GlobalVariable {
                        name,
                        original_name: None,
                        public_name,
                        public_arity,
                        external: false,
                        required: false,
                        params,
                        expr,
                        static_base_uri: decl.stylesheet_uri.clone(),
                    });
                }
                _ => {}
            }
        }
        Ok(globals)
    }

    pub(super) fn with_hidden_global_name<T, F>(&mut self, name: &ast::Name, f: F) -> error::SpannedResult<T>
    where
        F: FnOnce(&mut Self) -> error::SpannedResult<T>,
    {
        let hidden_name = self.variables.remove_var_name_in_current_scope(name);
        let result = f(self);
        if let Some(hidden_name) = hidden_name {
            self.variables
                .insert_var_name_in_current_scope(name.clone(), hidden_name);
        }
        result
    }

    pub(super) fn validate_param(&self, param: &ast::Param) -> error::SpannedResult<()> {
        if param.required && (param.select.is_some() || !param.sequence_constructor.is_empty()) {
            return Err(error::Error::XTSE0010.into());
        }
        if param.select.is_some()
            && Self::has_non_empty_binding_content(&param.sequence_constructor)
        {
            return Err(error::Error::XTSE0620.into());
        }
        Ok(())
    }

    pub(super) fn validate_variable(&self, variable: &ast::Variable) -> error::SpannedResult<()> {
        if variable.select.is_some()
            && Self::has_non_empty_binding_content(&variable.sequence_constructor)
        {
            return Err(error::Error::XTSE0620.into());
        }
        Ok(())
    }

    pub(super) fn has_non_empty_binding_content(sequence_constructor: &ast::SequenceConstructor) -> bool {
        sequence_constructor.iter().any(|item| match item {
            ast::SequenceConstructorItem::Content(ast::Content::Text(text)) => {
                !text.trim().is_empty()
            }
            _ => true,
        })
    }

    pub(super) fn global_variable_expr(
        &mut self,
        select: Option<&ast::Expression>,
        sequence_constructor: &ast::SequenceConstructor,
        sequence_type: Option<&xpath_ast::SequenceType>,
        span: ast::Span,
    ) -> error::SpannedResult<ir::ExprS> {
        let expr = if let Some(select) = select {
            self.expression(select)?.expr()
        } else if sequence_type.is_some() {
            self.sequence_constructor(sequence_constructor)?.expr()
        } else if !sequence_constructor.is_empty() {
            self.temporary_tree(sequence_constructor)?.expr()
        } else {
            Spanned::new(ir::Expr::Atom(self.empty_string()), adjusted_span(span, self.current_span_offset))
        };
        self.convert_expr(expr, sequence_type, RaisedError::XTTE0570, span)
    }

    pub(super) fn global_param_expr(
        &mut self,
        select: Option<&ast::Expression>,
        sequence_constructor: &ast::SequenceConstructor,
    ) -> error::SpannedResult<ir::ExprS> {
        let expr = if let Some(select) = select {
            self.expression(select)?.expr()
        } else if !sequence_constructor.is_empty() {
            self.sequence_constructor(sequence_constructor)?.expr()
        } else {
            self.empty_sequence()
        };
        Ok(expr)
    }

    pub(super) fn convert_expr(
        &mut self,
        expr: ir::ExprS,
        sequence_type: Option<&xpath_ast::SequenceType>,
        error: RaisedError,
        span: ast::Span,
    ) -> error::SpannedResult<ir::ExprS> {
        let Some(sequence_type) = sequence_type else {
            return Ok(expr);
        };

        let binding = self.variables.new_binding(expr.value, expr.span);
        let (atom, bindings) = Bindings::new(binding).atom_bindings();
        Ok(bindings
            .bind_expr_spanned(
                &mut self.variables,
                ir::Expr::ConvertSequence(ir::ConvertSequence {
                    atom,
                    sequence_type: sequence_type.clone(),
                    error,
                }),
                adjusted_span(span, self.current_span_offset),
            )
            .expr())
    }

    pub(super) fn convert_bindings(
        &mut self,
        bindings: Bindings,
        sequence_type: Option<&xpath_ast::SequenceType>,
        error: RaisedError,
        span: ast::Span,
    ) -> error::SpannedResult<Bindings> {
        let Some(sequence_type) = sequence_type else {
            return Ok(bindings);
        };

        let (atom, bindings) = bindings.atom_bindings();
        Ok(bindings.bind_expr_spanned(
            &mut self.variables,
            ir::Expr::ConvertSequence(ir::ConvertSequence {
                atom,
                sequence_type: sequence_type.clone(),
                error,
            }),
            adjusted_span(span, self.current_span_offset),
        ))
    }

    pub(super) fn with_param(
        &mut self,
        with_param: &ast::WithParam,
    ) -> error::SpannedResult<(ir::WithParam, Bindings)> {
        self.with_param_with_error(with_param, RaisedError::XTTE0570)
    }

    pub(super) fn evaluate_with_param(
        &mut self,
        with_param: &ast::WithParam,
    ) -> error::SpannedResult<(ir::WithParam, Bindings)> {
        self.with_param_with_error(with_param, RaisedError::XTTE0590)
    }

    pub(super) fn with_param_with_error(
        &mut self,
        with_param: &ast::WithParam,
        error: RaisedError,
    ) -> error::SpannedResult<(ir::WithParam, Bindings)> {
        if with_param.select.is_some()
            && Self::has_non_empty_binding_content(&with_param.sequence_constructor)
        {
            return Err(error::Error::XTSE0620.into());
        }
        let bindings = if let Some(select) = &with_param.select {
            self.expression(select)?
        } else if with_param.sequence_constructor.is_empty() {
            let expr = self.empty_sequence();
            Bindings::new(self.variables.new_binding(expr.value, expr.span))
        } else {
            self.sequence_constructor_with_temporary_output_state(&with_param.sequence_constructor)?
        };

        let bindings = self.convert_bindings(bindings, with_param.as_.as_ref(), error, with_param.span.clone())?;
        let (select_atom, bindings) = bindings.atom_bindings();

        Ok((
            ir::WithParam {
                name: ir::Name::new(with_param.name.local_name().to_string()),
                select: Some(select_atom),
                sequence_constructor: None,
                tunnel: with_param.tunnel,
            },
            bindings,
        ))
    }

    pub(super) fn context_params(context_names: &ir::ContextNames) -> Vec<ir::Param> {
        vec![
            ir::Param {
                name: context_names.item.clone(),
                type_: None,
                default: None,
                required: false,
                original_name: None,
                tunnel: false,
            },
            ir::Param {
                name: context_names.position.clone(),
                type_: None,
                default: None,
                required: false,
                original_name: None,
                tunnel: false,
            },
            ir::Param {
                name: context_names.last.clone(),
                type_: None,
                default: None,
                required: false,
                original_name: None,
                tunnel: false,
            },
        ]
    }

    pub(super) fn key(
        &mut self,
        declarations: &mut ir::Declarations,
        key: &ast::Key,
    ) -> error::SpannedResult<()> {
        // Compile the use expression (or sequence constructor) into a function
        // that takes a context node and returns the key value(s).
        let context_names = self.variables.push_context();
        let bindings = if let Some(use_expr) = &key.use_ {
            self.expression(use_expr)?
        } else if !key.sequence_constructor.is_empty() {
            self.sequence_constructor_with_temporary_output_state(&key.sequence_constructor)?
        } else {
            self.variables.pop_context();
            return Err(error::Error::Unsupported(
                "xsl:key without use= attribute or body is not yet supported".to_string(),
            )
            .into());
        };
        self.variables.pop_context();
        let use_function = ir::FunctionDefinition {
            declared_name: None,
            params: Self::context_params(&context_names),
            return_type: None,
            body: Box::new(bindings.expr()),
            static_base_uri: self.current_static_base_uri_string(),
        };

        // Compile the match pattern
        let pattern = transform_pattern(&key.match_.pattern, |expr| self.pattern_predicate(expr))?;

        let name = key.name.clone();
        declarations.keys.push(ir::KeyDefinition {
            name,
            pattern,
            use_function,
            composite: key.composite,
        });

        Ok(())
    }

    pub(super) fn accumulator(
        &mut self,
        accumulator: &ast::Accumulator,
    ) -> error::SpannedResult<ir::AccumulatorDefinition> {
        let mut rules = Vec::with_capacity(accumulator.rules.len());
        for rule in &accumulator.rules {
            rules.push(self.accumulator_rule(rule)?);
        }

        // Compile the initial-value expression as a zero-argument function
        let context_names = self.variables.push_context();
        let params = Self::context_params(&context_names);
        let initial_value_bindings = self.expression(&accumulator.initial_value)?;
        self.variables.pop_context();

        let initial_value = ir::FunctionDefinition {
            declared_name: None,
            params,
            return_type: None,
            body: Box::new(initial_value_bindings.expr()),
            static_base_uri: self.current_static_base_uri_string(),
        };

        Ok(ir::AccumulatorDefinition {
            name: accumulator.name.clone(),
            initial_value,
            rules,
        })
    }

    pub(super) fn accumulator_rule(
        &mut self,
        rule: &ast::AccumulatorRule,
    ) -> error::SpannedResult<ir::AccumulatorRuleDefinition> {
        let context_names = self.variables.push_context();
        self.variables.push_scope();

        let value_original_name = OwnedName::new("value".to_string(), String::new(), String::new());
        let value_name = self.variables.declare_var_name(&value_original_name);

        let bindings = if let Some(select) = &rule.select {
            self.expression(select)?
        } else if !rule.sequence_constructor.is_empty() {
            self.sequence_constructor_with_temporary_output_state(&rule.sequence_constructor)?
        } else {
            Bindings::empty()
        };

        self.variables.pop_scope();
        self.variables.pop_context();

        let mut params = Self::context_params(&context_names);
        params.push(ir::Param {
            name: value_name,
            type_: None,
            default: None,
            required: false,
            original_name: Some("value".to_string()),
            tunnel: false,
        });

        let pattern = transform_pattern(&rule.match_.pattern, |expr| self.pattern_predicate(expr))?;
        let phase = match rule.phase {
            Some(ast::AccumulatorPhase::End) => ir::AccumulatorPhase::End,
            Some(ast::AccumulatorPhase::Start) | None => ir::AccumulatorPhase::Start,
        };

        Ok(ir::AccumulatorRuleDefinition {
            pattern,
            phase,
            probe_temporary_output_state: !rule.sequence_constructor.is_empty(),
            rule_function: ir::FunctionDefinition {
                declared_name: None,
                params,
                return_type: None,
                body: Box::new(bindings.expr()),
                static_base_uri: self.current_static_base_uri_string(),
            },
        })
    }

    /// Compile an xsl:template into one or more IR rules (for match templates)
    /// and/or a named function binding (for named templates).
    ///
    /// When a template has both @match and @name, it produces both a rule
    /// and a function binding. When @priority is absent, the template is split
    /// into one rule per default-priority alternative in the match pattern.
    pub(super) fn template(
        &mut self,
        declarations: &mut ir::Declarations,
        template: &ast::Template,
        import_precedence: i64,
        module_path: &[usize],
    ) -> error::SpannedResult<()> {
        for param in &template.params {
            self.validate_param(param)?;
        }
        let named_template_function = if let Some(name) = &template.name {
            Some(ir::FunctionBinding {
                name: ir::Name::new(name.local_name().to_string()),
                import_precedence,
                main: self.template_with_params_function(template)?,
            })
        } else {
            None
        };

        if let Some(pattern) = &template.match_ {
            let function_definition = self.matched_template_function(template)?;
            let modes = template
                .mode
                .iter()
                .map(Self::ast_mode_value_to_ir_mode_value)
                .collect::<Vec<_>>();

            if let Some(priority) = &template.priority {
                declarations.rules.push(ir::Rule {
                    import_precedence,
                    module_path: module_path.to_vec(),
                    priority: *priority,
                    modes,
                    pattern: transform_pattern(&pattern.pattern, |expr| {
                        self.pattern_predicate(expr)
                    })?,
                    function_definition,
                });
                if let Some(function_binding) = named_template_function {
                    declarations.functions.push(function_binding);
                }
                return Ok(());
            }

            let default_priorities = default_priority(&pattern.pattern).collect::<Vec<_>>();
            for (split_pattern, priority) in default_priorities {
                declarations.rules.push(ir::Rule {
                    import_precedence,
                    module_path: module_path.to_vec(),
                    priority,
                    modes: modes.clone(),
                    pattern: transform_pattern(&split_pattern, |expr| {
                        self.pattern_predicate(expr)
                    })?,
                    function_definition: function_definition.clone(),
                });
            }
            if let Some(function_binding) = named_template_function {
                declarations.functions.push(function_binding);
            }
            Ok(())
        } else if let Some(function_binding) = named_template_function {
            declarations.functions.push(function_binding);
            Ok(())
        } else {
            Err(error::Error::Unsupported(
                "Template must have either match or name attribute".to_string(),
            )
            .into())
        }
    }

    pub(super) fn template_with_params_function(
        &mut self,
        template: &ast::Template,
    ) -> error::SpannedResult<ir::FunctionDefinition> {
        self.with_template_continuation_availability(true, |this| {
            let context_names = this.template_context_names(template);
            this.variables.push_scope();
            let param_names = this.register_template_param_names(template)?;

            let bindings = this.sequence_constructor(&template.sequence_constructor)?;
            let mut params = Self::context_params(&context_names);
            params.extend(this.template_params(template, param_names)?);
            this.variables.pop_scope();
            this.variables.pop_context();

            Ok(ir::FunctionDefinition {
                declared_name: None,
                params,
                return_type: None,
                body: Box::new(bindings.expr()),
                static_base_uri: this.current_static_base_uri_string(),
            })
        })
    }

    pub(super) fn matched_template_function(
        &mut self,
        template: &ast::Template,
    ) -> error::SpannedResult<ir::FunctionDefinition> {
        self.with_template_continuation_availability(true, |this| {
            let context_names = this.template_context_names(template);
            this.variables.push_scope();
            let param_names = this.register_template_param_names(template)?;
            let bindings = this.sequence_constructor(&template.sequence_constructor)?;

            let mut params = Self::context_params(&context_names);
            params.extend(this.template_params(template, param_names)?);
            this.variables.pop_scope();
            this.variables.pop_context();

            Ok(ir::FunctionDefinition {
                declared_name: None,
                params,
                return_type: None,
                body: Box::new(bindings.expr()),
                static_base_uri: this.current_static_base_uri_string(),
            })
        })
    }

    pub(super) fn template_context_names(&mut self, template: &ast::Template) -> ir::ContextNames {
        let has_absent_context = matches!(
            template
                .context_item
                .as_ref()
                .and_then(|context_item| context_item.use_.as_ref()),
            Some(ast::Use::Absent)
        );
        if has_absent_context {
            let item_name = self.variables.new_name();
            let context_names = self.variables.explicit_context_names(item_name);
            self.variables.push_absent_context();
            context_names
        } else {
            self.variables.push_context()
        }
    }

    pub(super) fn xslt_function_definition(
        &mut self,
        function: &ast::Function,
    ) -> error::SpannedResult<ir::FunctionDefinition> {
        self.variables.push_absent_context();
        self.variables.push_scope();

        let mut params = Vec::new();
        let mut seen_names = HashSet::new();
        for param in &function.params {
            let param_key = (
                param.name.namespace().to_string(),
                param.name.local_name().to_string(),
            );
            if !seen_names.insert(param_key) {
                return Err(error::Error::Unsupported(
                    "Duplicate XSLT function parameters are not supported".to_string(),
                )
                .into());
            }

            let name = self.variables.declare_var_name(&param.name);
            params.push(ir::Param {
                name,
                type_: param.as_.clone(),
                default: None,
                required: true,
                original_name: None,
                tunnel: false,
            });
        }

        let bindings = self.sequence_constructor_with_temporary_output_state(
            &function.sequence_constructor,
        )?;

        // Enforce the declared return type (as="...") by wrapping the body
        // with a ConvertSequence check. Without this, a function returning
        // the wrong type silently passes the unchecked value to callers.
        let bindings = self.convert_bindings(
            bindings,
            function.as_.as_ref(),
            RaisedError::XTTE0570,
            function.span,
        )?;

        self.variables.pop_scope();
        self.variables.pop_context();

        Ok(ir::FunctionDefinition {
            declared_name: Some(function.name.clone()),
            params,
            return_type: function.as_.clone(),
            body: Box::new(bindings.expr()),
            static_base_uri: self.current_static_base_uri_string(),
        })
    }

    pub(super) fn register_template_param_names(
        &mut self,
        template: &ast::Template,
    ) -> error::SpannedResult<Vec<(String, ir::Name)>> {
        let mut param_names = Vec::new();
        let mut seen_names = HashSet::new();
        for param in &template.params {
            let param_key = (
                param.name.namespace().to_string(),
                param.name.local_name().to_string(),
            );
            if !seen_names.insert(param_key) {
                return Err(error::SpannedError {
                    error: error::Error::XTSE0580,
                    span: Some(adjusted_span(param.span, self.current_span_offset).into()),
                    detail: None,

                    contexts: Vec::new(),
                });
            }
            let var_name = self.variables.declare_var_name(&param.name);
            param_names.push((param.name.local_name().to_string(), var_name));
        }
        Ok(param_names)
    }

    pub(super) fn template_params(
        &mut self,
        template: &ast::Template,
        param_names: Vec<(String, ir::Name)>,
    ) -> error::SpannedResult<Vec<ir::Param>> {
        let mut params = Vec::new();
        for (original_name, runtime_name) in param_names {
            let ast_param = template
                .params
                .iter()
                .find(|param| param.name.local_name() == original_name);
            let required = ast_param.map(|param| param.required).unwrap_or(false);
            let param_type = ast_param.and_then(|param| param.as_.clone());

            let default = if let Some(ast_param) = ast_param {
                if !ast_param.sequence_constructor.is_empty() {
                    let expr_s = self
                        .sequence_constructor_with_temporary_output_state(
                            &ast_param.sequence_constructor,
                        )?
                        .expr();
                    let expr_s =
                        self.convert_expr(expr_s, ast_param.as_.as_ref(), RaisedError::XTTE0590, ast_param.span.clone())?;
                    Some(Box::new(expr_s.value))
                } else if let Some(select_expr) = &ast_param.select {
                    let expr_s = self.expression(select_expr)?.expr();
                    let expr_s =
                        self.convert_expr(expr_s, ast_param.as_.as_ref(), RaisedError::XTTE0590, ast_param.span.clone())?;
                    Some(Box::new(expr_s.value))
                } else {
                    None
                }
            } else {
                None
            };

            params.push(ir::Param {
                name: runtime_name,
                type_: param_type,
                default,
                required,
                original_name: Some(original_name),
                tunnel: ast_param.map(|param| param.tunnel).unwrap_or(false),
            });
        }
        Ok(params)
    }

    pub(super) fn mode(
        &mut self,
        declarations: &mut ir::Declarations,
        mode: &ast::Mode,
    ) -> error::SpannedResult<()> {
        declarations.modes.insert(
            mode.name.clone(),
            ir::Mode {
                on_no_match: match mode.on_no_match.as_ref() {
                    Some(ast::OnNoMatch::DeepCopy) => ir::ModeOnNoMatch::DeepCopy,
                    Some(ast::OnNoMatch::ShallowCopy) => ir::ModeOnNoMatch::ShallowCopy,
                    Some(ast::OnNoMatch::DeepSkip) => ir::ModeOnNoMatch::DeepSkip,
                    Some(ast::OnNoMatch::ShallowSkip) => ir::ModeOnNoMatch::ShallowSkip,
                    Some(ast::OnNoMatch::Fail) => ir::ModeOnNoMatch::Fail,
                    Some(ast::OnNoMatch::TextOnlyCopy) | None => ir::ModeOnNoMatch::TextOnlyCopy,
                },
                on_multiple_match: match mode.on_multiple_match.as_ref() {
                    Some(ast::OnMultipleMatch::Fail) => ir::OnMultipleMatch::Fail,
                    Some(ast::OnMultipleMatch::UseLast) | None => ir::OnMultipleMatch::UseLast,
                },
                warning_on_no_match: mode.warning_on_no_match,
                typed: match mode.typed.as_ref() {
                    Some(ast::Typed::Yes) => ir::ModeTyped::Yes,
                    Some(ast::Typed::Strict) => ir::ModeTyped::Strict,
                    Some(ast::Typed::Lax) => ir::ModeTyped::Lax,
                    Some(ast::Typed::No) | Some(ast::Typed::Unspecified) | None => {
                        ir::ModeTyped::No
                    }
                },
            },
        );
        Ok(())
    }

    /// Compile the unnamed xsl:output declaration into serialization parameters.
    /// Named outputs are stored separately and applied by xsl:result-document.
    pub(super) fn output(
        &mut self,
        declarations: &mut ir::Declarations,
        output: &ast::Output,
    ) -> error::SpannedResult<()> {
        if output.name.is_some() {
            return Ok(());
        }
        let serialization = &mut declarations.serialization_params;
        if let Some(parameter_document) = &output.parameter_document {
            let loaded = self.load_output_parameter_document(parameter_document)?;
            if let Some(method) = loaded.method {
                serialization.method = QNameOrString::String(method);
            }
            if !loaded.use_character_maps.is_empty() {
                serialization.use_character_maps = loaded.use_character_maps;
            }
        }
        if output.build_tree {
            return Err(error::Error::Unsupported(String::from(
                "Output: Build tree is not supported yet",
            ))
            .into());
        }
        fn assign_if_some<T>(location: &mut T, value: Option<T>) {
            if let Some(v) = value {
                *location = v;
            }
        }
        serialization.allow_duplicate_names = output.allow_duplicate_names;
        serialization.byte_order_mark = output.byte_order_mark;
        serialization
            .cdata_section_elements
            .extend(output.cdata_section_elements.clone());
        serialization.doctype_public = output.doctype_public.clone();
        serialization.doctype_system = output.doctype_system.clone();
        match &output.method {
            Some(ast::OutputMethod::Adaptive) => {
                serialization.method = QNameOrString::String("adaptive".to_string());
                serialization.explicit_method = true;
            }
            Some(ast::OutputMethod::Xml) => {
                serialization.method = QNameOrString::String("xml".to_string());
                serialization.explicit_method = true;
            }
            Some(ast::OutputMethod::Html) => {
                serialization.method = QNameOrString::String("html".to_string());
                serialization.explicit_method = true;
            }
            Some(ast::OutputMethod::Xhtml) => {
                serialization.method = QNameOrString::String("xhtml".to_string());
                serialization.explicit_method = true;
            }
            Some(ast::OutputMethod::Text) => {
                serialization.method = QNameOrString::String("text".to_string());
                serialization.explicit_method = true;
            }
            Some(ast::OutputMethod::Json) => {
                serialization.method = QNameOrString::String("json".to_string());
                serialization.explicit_method = true;
            }
            None => {}
            method => {
                return Err(error::Error::Unsupported(format!(
                    "Output method {:?} not supported yet",
                    method
                ))
                .into());
            }
        };
        assign_if_some(&mut serialization.encoding, output.encoding.clone());
        serialization.escape_uri_attributes = output.escape_uri_attributes;
        assign_if_some(&mut serialization.html_version, output.html_version);
        serialization.explicit_html_version = output.html_version.is_some();
        serialization.include_content_type = output.include_content_type;
        serialization.indent = output.indent;
        assign_if_some(
            &mut serialization.item_separator,
            output.item_separator.clone(),
        );
        match &output.json_node_output_method {
            Some(ast::JsonNodeOutputMethod::Xml) => {
                serialization.json_node_output_method = QNameOrString::String("xml".to_string())
            }
            Some(ast::JsonNodeOutputMethod::Html) => {
                serialization.json_node_output_method = QNameOrString::String("html".to_string())
            }
            None => {}
            method => {
                return Err(error::Error::Unsupported(format!(
                    "JSON node output method {:?} not supported yet",
                    method
                ))
                .into());
            }
        }
        serialization.media_type = output.media_type.clone();
        serialization.normalization_form =
            output.normalization_form.as_ref().and_then(|nf| match nf {
                ast::NormalizationForm::Nfc => Some(String::from("NFC")),
                ast::NormalizationForm::Nfd => Some(String::from("NFD")),
                ast::NormalizationForm::Nfkc => Some(String::from("NFKC")),
                ast::NormalizationForm::Nfkd => Some(String::from("NFKD")),
                ast::NormalizationForm::FullyNormalized => Some(String::from("fully-normalized")),
                ast::NormalizationForm::NmToken(nm) => Some(nm.clone()),
                ast::NormalizationForm::None => None,
            });
        serialization.omit_xml_declaration = output.omit_xml_declaration.unwrap_or_else(|| {
            // For XHTML with html-version >= 5 and HTML, the XML declaration
            // SHOULD NOT be output. Our default html-version is 5.0.
            match &output.method {
                Some(ast::OutputMethod::Xhtml)
                    if serialization.html_version
                        >= Decimal::from_str_exact("5.0").unwrap() =>
                {
                    true
                }
                Some(ast::OutputMethod::Html) => true,
                _ => false,
            }
        });
        // Per spec, if standalone is explicitly set (yes, no, or omit),
        // the XML declaration MUST be output. The standalone attribute
        // is a pseudo-attribute of the XML declaration, so its presence
        // implies the declaration should be emitted.
        if output.standalone.is_some() {
            serialization.omit_xml_declaration = false;
        }
        assign_if_some(
            &mut serialization.standalone,
            output.standalone.as_ref().map(|s| match s {
                ast::Standalone::Bool(b) => Some(*b),
                ast::Standalone::Omit => None,
            }),
        );
        serialization
            .suppress_indentation
            .extend(output.suppress_indentation.clone());
        serialization.undeclare_prefixes = output.undeclare_prefixes;
        if !output.use_character_maps.is_empty() {
            serialization.use_character_maps =
                self.resolve_character_maps(&output.use_character_maps)?;
        }
        assign_if_some(&mut serialization.version, output.version.clone());
        Ok(())
    }

    pub(super) fn load_output_parameter_document(
        &self,
        parameter_document: &str,
    ) -> error::SpannedResult<LoadedOutputParameterDocument> {
        let resolved = self
            .resolve_static_base_uri(parameter_document)
            .ok_or_else(|| error::Error::Unsupported(format!(
                "Output: Could not resolve parameter document {parameter_document}"
            )))?;
        let resolved = resolved.to_string();
        let Some(path) = resolved.strip_prefix("file://") else {
            return Err(error::Error::Unsupported(format!(
                "Output: Parameter documents currently require file URIs, got {resolved}"
            ))
            .into());
        };

        let xml = std::fs::read_to_string(path).map_err(|error| {
            error::Error::Unsupported(format!(
                "Output: Could not read parameter document {parameter_document}: {error}"
            ))
        })?;

        let mut xot = Xot::new();
        let document = xot.parse(&xml).map_err(|error| {
            error::Error::Unsupported(format!(
                "Output: Could not parse parameter document {parameter_document}: {error}"
            ))
        })?;
        let root = xot.document_element(document).map_err(|error| {
            error::Error::Unsupported(format!(
                "Output: Parameter document {parameter_document} has no document element: {error}"
            ))
        })?;

        let Some(root_name) = xot.node_name(root) else {
            return Err(error::Error::Unsupported(format!(
                "Output: Parameter document {parameter_document} has no root name"
            ))
            .into());
        };
        let (local_name, namespace) = xot.name_ns_str(root_name);
        if local_name != "serialization-parameters" || namespace != SERIALIZATION_NAMESPACE {
            return Err(error::Error::Unsupported(format!(
                "Output: Unsupported parameter document root {{{namespace}}}{local_name}"
            ))
            .into());
        }

        let value_name = xot.add_name("value");
        let character_name = xot.add_name("character");
        let map_string_name = xot.add_name("map-string");

        let mut loaded = LoadedOutputParameterDocument::default();
        for child in xot.children(root).filter(|node| xot.is_element(*node)) {
            let Some(child_name) = xot.node_name(child) else {
                continue;
            };
            let (local_name, namespace) = xot.name_ns_str(child_name);
            if namespace != SERIALIZATION_NAMESPACE {
                continue;
            }

            match local_name {
                "method" => {
                    if let Some(value) = xot.attributes(child).get(value_name) {
                        loaded.method = Some(value.to_string());
                    }
                }
                "use-character-maps" => {
                    for map_node in xot.children(child).filter(|node| xot.is_element(*node)) {
                        let Some(map_name) = xot.node_name(map_node) else {
                            continue;
                        };
                        let (local_name, namespace) = xot.name_ns_str(map_name);
                        if local_name != "character-map" || namespace != SERIALIZATION_NAMESPACE {
                            continue;
                        }

                        let Some(character) = xot.attributes(map_node).get(character_name) else {
                            continue;
                        };
                        let Some(map_string) = xot.attributes(map_node).get(map_string_name) else {
                            continue;
                        };
                        let Some(character) = character.chars().next() else {
                            continue;
                        };
                        loaded.use_character_maps.insert(character, map_string.to_string());
                    }
                }
                _ => {}
            }
        }

        Ok(loaded)
    }

    pub(super) fn ast_mode_value_to_ir_mode_value(mode: &ast::ModeValue) -> ir::ModeValue {
        match mode {
            ast::ModeValue::EqName(name) => ir::ModeValue::Named(name.clone()),
            ast::ModeValue::Unnamed => ir::ModeValue::Unnamed,
            ast::ModeValue::All => ir::ModeValue::All,
        }
    }
}
