use ahash::{HashMap, HashMapExt};
use rust_decimal::Decimal;
use xee_xpath_ast::Pattern;

use crate::{function, pattern::ModeId, pattern::ModeLookup};
use crate::sequence::SerializationParameters;
use xot::xmlname::OwnedName;

#[derive(Debug, Clone)]
pub struct GlobalVariableDeclaration {
    pub name: function::Name,
    pub function_id: function::InlineFunctionId,
    pub original_name: Option<OwnedName>,
    pub external: bool,
    pub required: bool,
}

#[derive(Debug, Clone)]
pub struct NamedTemplateDeclaration {
    pub name: function::Name,
    pub function_id: function::InlineFunctionId,
}

#[derive(Debug, Clone)]
pub struct TemplateParamDeclaration {
    pub name: String,
    pub tunnel: bool,
}

#[derive(Debug, Clone)]
pub struct KeyDeclaration {
    pub name: OwnedName,
    pub pattern: Pattern<function::InlineFunctionId>,
    pub use_function_id: function::InlineFunctionId,
}

/// A compiled count or from pattern for xsl:number, stored in Declarations
/// so runtime functions can access it by index.
#[derive(Debug, Clone)]
pub struct NumberPatternDeclaration {
    pub pattern: Pattern<function::InlineFunctionId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeDeclaration {
    pub on_no_match: ModeOnNoMatch,
    pub on_multiple_match: Option<OnMultipleMatch>,
    pub warning_on_no_match: bool,
    pub typed: ModeTyped,
}

impl Default for ModeDeclaration {
    fn default() -> Self {
        Self {
            on_no_match: ModeOnNoMatch::TextOnlyCopy,
            on_multiple_match: None,
            warning_on_no_match: false,
            typed: ModeTyped::No,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeOnNoMatch {
    DeepCopy,
    ShallowCopy,
    DeepSkip,
    ShallowSkip,
    TextOnlyCopy,
    Fail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnMultipleMatch {
    UseLast,
    Fail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeTyped {
    Yes,
    No,
    Strict,
    Lax,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateRule {
    pub function_id: function::InlineFunctionId,
    pub import_precedence: i64,
    pub priority: Decimal,
    pub module_path: Vec<usize>,
}

#[derive(Debug)]
pub struct Declarations {
    pub mode_lookup: ModeLookup<TemplateRule>,
    modes: HashMap<ModeId, ModeDeclaration>,
    pub global_variables: Vec<GlobalVariableDeclaration>,
    pub named_templates: Vec<NamedTemplateDeclaration>,
    pub keys: Vec<KeyDeclaration>,
    pub number_patterns: Vec<NumberPatternDeclaration>,
    pub serialization_params: SerializationParameters,
    template_params: HashMap<function::InlineFunctionId, Vec<TemplateParamDeclaration>>,
    template_import_precedence: HashMap<function::InlineFunctionId, i64>,
    template_module_path: HashMap<function::InlineFunctionId, Vec<usize>>,
}

impl Declarations {
    pub(crate) fn new() -> Self {
        Self {
            mode_lookup: ModeLookup::new(),
            modes: HashMap::new(),
            global_variables: Vec::new(),
            named_templates: Vec::new(),
            keys: Vec::new(),
            number_patterns: Vec::new(),
            serialization_params: SerializationParameters::new(),
            template_params: HashMap::new(),
            template_import_precedence: HashMap::new(),
            template_module_path: HashMap::new(),
        }
    }

    pub fn add_mode(&mut self, mode_id: ModeId, declaration: ModeDeclaration) {
        self.modes.insert(mode_id, declaration);
    }

    pub fn mode(&self, mode_id: ModeId) -> ModeDeclaration {
        self.modes.get(&mode_id).copied().unwrap_or_default()
    }

    pub fn add_global_variable(&mut self, global_variable: GlobalVariableDeclaration) {
        self.global_variables.push(global_variable);
    }

    pub fn global_variable(&self, index: usize) -> &GlobalVariableDeclaration {
        &self.global_variables[index]
    }

    pub fn add_named_template(&mut self, named_template: NamedTemplateDeclaration) {
        self.named_templates.push(named_template);
    }

    pub fn named_template(&self, index: usize) -> &NamedTemplateDeclaration {
        &self.named_templates[index]
    }

    pub fn named_template_by_name(&self, name: &str) -> Option<&NamedTemplateDeclaration> {
        self.named_templates
            .iter()
            .find(|named_template| named_template.name == function::Name::new(name.to_string()))
    }

    pub fn add_template_params(
        &mut self,
        function_id: function::InlineFunctionId,
        params: Vec<TemplateParamDeclaration>,
    ) {
        self.template_params.insert(function_id, params);
    }

    pub fn template_params(
        &self,
        function_id: function::InlineFunctionId,
    ) -> Option<&[TemplateParamDeclaration]> {
        self.template_params.get(&function_id).map(Vec::as_slice)
    }

    pub fn add_template_import_precedence(
        &mut self,
        function_id: function::InlineFunctionId,
        import_precedence: i64,
    ) {
        self.template_import_precedence
            .insert(function_id, import_precedence);
    }

    pub fn template_import_precedence(
        &self,
        function_id: function::InlineFunctionId,
    ) -> Option<i64> {
        self.template_import_precedence.get(&function_id).copied()
    }

    pub fn template_import_precedences(&self) -> &HashMap<function::InlineFunctionId, i64> {
        &self.template_import_precedence
    }

    pub fn add_template_module_path(
        &mut self,
        function_id: function::InlineFunctionId,
        module_path: Vec<usize>,
    ) {
        self.template_module_path.insert(function_id, module_path);
    }

    pub fn template_module_path(
        &self,
        function_id: function::InlineFunctionId,
    ) -> Option<&[usize]> {
        self.template_module_path.get(&function_id).map(Vec::as_slice)
    }

    pub fn template_module_paths(
        &self,
    ) -> &HashMap<function::InlineFunctionId, Vec<usize>> {
        &self.template_module_path
    }

    pub fn add_key(&mut self, key: KeyDeclaration) {
        self.keys.push(key);
    }

    pub fn keys_by_name(&self, name: &OwnedName) -> Vec<&KeyDeclaration> {
        self.keys.iter().filter(|k| &k.name == name).collect()
    }

    pub fn add_number_pattern(&mut self, pattern: NumberPatternDeclaration) -> usize {
        let index = self.number_patterns.len();
        self.number_patterns.push(pattern);
        index
    }

    pub fn number_pattern(&self, index: usize) -> &NumberPatternDeclaration {
        &self.number_patterns[index]
    }
}
