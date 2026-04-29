use std::cell::OnceCell;
use std::fs;
use std::rc::Rc;

use ahash::HashMap;
use iri_string::types::{IriReferenceStr, IriString};
use xee_xpath_ast::{ast, pattern, Pattern};
use xot::xmlname::NameStrInfo;
use xot::Xot;

use crate::function;
use crate::interpreter::Interpreter;
use crate::pattern::pattern_core::PredicateMatcher;
use crate::sequence::{Item, Sequence};

/// Cache mapping pointer-as-integer of `OwnedName` instances in pattern ASTs
/// to their resolved `NameId`. Keyed by raw pointer cast to `usize` for
/// O(1) integer-key lookup — avoids string hashing.
/// Only successfully resolved names are cached; unresolvable names (e.g.
/// from documents not yet loaded via `doc()`) are omitted so they fall
/// back to direct resolution at match time.
pub(crate) type NameCache = HashMap<usize, xot::NameId>;

#[derive(Debug, Clone, Default)]
pub struct PatternLookup<V: Clone> {
    pub(crate) patterns: Vec<(Pattern<function::InlineFunctionId>, V)>,
    /// Lazily built index mapping NameId -> indices into `patterns`.
    name_index: OnceCell<NameIndex>,
}

/// Index that partitions patterns into name-specific buckets and a wildcard
/// list.  Built once (lazily) from the flat patterns Vec by resolving
/// OwnedName → NameId via &Xot.  Indices refer into the patterns Vec which
/// is already sorted by priority (highest first).
#[derive(Debug, Clone, Default)]
struct NameIndex {
    /// Patterns whose anchor step is a specific element/attribute name test.
    by_name: HashMap<xot::NameId, Vec<usize>>,
    /// Patterns that can match any name (wildcard, kind-test, predicate-only,
    /// union, etc.) — these must always be checked.
    wildcards: Vec<usize>,
    /// Pre-resolved OwnedName → NameId cache for ALL name tests across ALL
    /// patterns, keyed by pointer-as-integer. Built once during
    /// `build_index`; used during pattern matching to skip repeated
    /// OwnedName::maybe_to_ref lookups (which do 3 hash probes each).
    name_cache: Rc<NameCache>,
}

pub(crate) struct InterpreterPredicateMatcher<'a> {
    interpreter: &'a mut Interpreter<'a>,
}

impl<'a> InterpreterPredicateMatcher<'a> {
    pub(crate) fn new(interpreter: &'a mut Interpreter<'a>) -> Self {
        Self { interpreter }
    }
}

impl PredicateMatcher for Interpreter<'_> {
    fn match_predicate_with_context(
        &mut self,
        inline_function_id: function::InlineFunctionId,
        item: &Item,
        position: usize,
        size: usize,
    ) -> bool {
        let function = function::InlineFunctionData::new(inline_function_id, Vec::new()).into();
        let arguments = vec![
            item.clone().into(),
            (position as u64).into(),
            (size as u64).into(),
        ];
        let checkpoint = self.state.checkpoint();

        // the specification says to swallow any errors
        // TODO: log errors somehow here?
        let value = self.call_function_with_arguments(&function, arguments);
        if let Ok(value) = value {
            value.effective_boolean_value().unwrap_or(false)
        } else {
            self.state.restore(checkpoint);
            false
        }
    }

    fn xot(&self) -> &Xot {
        self.xot()
    }

    fn resolve_name(&self, name: &xot::xmlname::OwnedName) -> Option<xot::NameId> {
        if let Some(cache) = &self.name_cache {
            let key = name as *const _ as usize;
            if let Some(&name_id) = cache.get(&key) {
                return Some(name_id);
            }
        }
        // Fallback: resolve directly (for patterns not in the cache,
        // e.g. xsl:number count patterns or names from documents loaded
        // after the cache was built).
        let namespace_id = self.xot().namespace(name.namespace())?;
        self.xot().name_ns(name.local_name(), namespace_id)
    }

    fn rooted_pattern_sequence(&mut self, root: &pattern::RootExpr) -> Option<Sequence> {
        match root {
            pattern::RootExpr::VarRef(name) => self.lookup_root_variable(name),
            pattern::RootExpr::FunctionCall(function_call) => {
                self.lookup_root_function(function_call)
            }
        }
    }
}

impl Interpreter<'_> {
    fn lookup_root_variable(&mut self, name: &ast::Name) -> Option<Sequence> {
        if let Some(sequence) = self.runnable().dynamic_context().variables().get(name) {
            return Some(sequence.clone());
        }

        let declarations = &self.runnable().program().declarations;
        declarations
            .global_variables
            .iter()
            .position(|global| global.original_name.as_ref() == Some(name))
            .and_then(|index| self.resolve_global_variable(index).ok())
    }

    fn lookup_root_function(&mut self, function_call: &pattern::FunctionCall) -> Option<Sequence> {
        match function_call.name {
            pattern::OuterFunctionName::Doc => self.lookup_root_doc(function_call),
            pattern::OuterFunctionName::Id
            | pattern::OuterFunctionName::ElementWithId
            | pattern::OuterFunctionName::Key
            | pattern::OuterFunctionName::Root => None,
        }
    }

    fn lookup_root_doc(&mut self, function_call: &pattern::FunctionCall) -> Option<Sequence> {
        let [uri_argument] = function_call.args.as_slice() else {
            return None;
        };
        let uri = self.resolve_pattern_argument_string(uri_argument)?;
        let document_node = self.load_pattern_document(&uri)?;
        Some(Sequence::from(vec![Item::from(document_node)]))
    }

    fn resolve_pattern_argument_string(&mut self, argument: &pattern::Argument) -> Option<String> {
        match argument {
            pattern::Argument::Literal(literal) => Some(literal_to_string(literal)),
            pattern::Argument::VarRef(name) => self
                .lookup_root_variable(name)
                .and_then(|sequence| sequence.string_value(self.xot()).ok()),
        }
    }

    fn load_pattern_document(&mut self, uri: &str) -> Option<xot::Node> {
        let iri_reference: &IriReferenceStr = uri.try_into().ok()?;
        let uri = self.absolute_pattern_uri(iri_reference)?;

        let documents = self.runnable().dynamic_context().documents();
        if let Some(document) = documents.borrow().get_by_uri(&uri) {
            return Some(document.root());
        }

        let url = url::Url::parse(uri.as_str()).ok()?;
        let path = url.to_file_path().ok()?;
        let xml = fs::read_to_string(&path).ok()?;

        let handle = documents
            .borrow_mut()
            .add_string(self.xot_mut(), Some(uri.as_ref()), &xml)
            .ok()?;
        let node = documents.borrow().get_node_by_handle(handle);
        node
    }

    fn absolute_pattern_uri(&self, uri: &IriReferenceStr) -> Option<IriString> {
        match uri.to_iri() {
            Ok(iri) => Some(iri.into()),
            Err(relative_iri) => {
                let base = self
                    .runnable()
                    .dynamic_context()
                    .static_context()
                    .static_base_uri()?;
                Some(relative_iri.resolve_against(base).into())
            }
        }
    }
}

fn literal_to_string(literal: &ast::Literal) -> String {
    match literal {
        ast::Literal::Decimal(value) => value.to_string(),
        ast::Literal::Integer(value) => value.to_string(),
        ast::Literal::Double(value) => value.to_string(),
        ast::Literal::String(value) => value.clone(),
    }
}

impl<V: Clone> PatternLookup<V> {
    pub(crate) fn new() -> Self {
        Self {
            patterns: Vec::new(),
            name_index: OnceCell::new(),
        }
    }

    pub(crate) fn add_rules(&mut self, rules: Vec<(Pattern<function::InlineFunctionId>, V)>) {
        self.patterns.extend(rules);
        // OnceCell doesn't need explicit invalidation since add_rules is only
        // called during compilation, before any lookups occur.
    }

    /// Build the name index from the current patterns using `xot` to resolve
    /// OwnedName → NameId.  Called lazily on first lookup via OnceCell.
    fn build_index(&self, xot: &Xot) -> NameIndex {
        let mut by_name: HashMap<xot::NameId, Vec<usize>> = HashMap::default();
        let mut wildcards: Vec<usize> = Vec::new();

        for (i, (pattern, _)) in self.patterns.iter().enumerate() {
            match extract_anchor_name(pattern) {
                AnchorName::Name(owned_name) => {
                    if let Some(name_id) = resolve_owned_name(owned_name, xot) {
                        by_name.entry(name_id).or_default().push(i);
                    } else {
                        // Name not found in xot — can never match, but keep
                        // in wildcards for correctness
                        wildcards.push(i);
                    }
                }
                AnchorName::Wildcard => {
                    wildcards.push(i);
                }
                AnchorName::Multiple(names) => {
                    // Union pattern with multiple specific names — add to each
                    // bucket, but only once per NameId to avoid duplicate
                    // indices that break lookup_after
                    let mut added_any = false;
                    let mut seen_name_ids: Vec<xot::NameId> = Vec::new();
                    for name in names {
                        if let Some(name_id) = resolve_owned_name(&name, xot) {
                            if !seen_name_ids.contains(&name_id) {
                                seen_name_ids.push(name_id);
                                by_name.entry(name_id).or_default().push(i);
                            }
                            added_any = true;
                        }
                    }
                    if !added_any {
                        wildcards.push(i);
                    }
                }
            }
        }

        // Build the name cache: resolve ALL NameTest::Name OwnedNames across
        // ALL patterns to NameId. This cache is used during pattern matching
        // to skip the expensive OwnedName::maybe_to_ref (3 hash probes per call).
        let mut name_cache: NameCache = NameCache::default();
        for (pattern, _) in &self.patterns {
            collect_pattern_names(pattern, xot, &mut name_cache);
        }

        NameIndex {
            by_name,
            wildcards,
            name_cache: Rc::new(name_cache),
        }
    }

    /// Ensure the name index is built. Call this while `xot` is available
    /// (before any mutable borrow of `xot` begins) so that later lookups
    /// don't need an `&Xot` reference.
    pub(crate) fn ensure_index(&self, xot: &Xot) {
        self.name_index.get_or_init(|| self.build_index(xot));
    }

    /// Get the pre-resolved name cache as an Rc (cheap clone).
    /// Returns None if the index hasn't been built yet.
    pub(crate) fn name_cache_rc(&self) -> Option<Rc<NameCache>> {
        self.name_index.get().map(|idx| Rc::clone(&idx.name_cache))
    }

    /// Iterate pattern indices relevant for a node with the given optional
    /// NameId, yielding them in priority order (= original Vec order).
    fn relevant_indices(&self, name_id: Option<xot::NameId>) -> MergedIndices<'_> {
        let index = self.name_index.get().expect("ensure_index must be called before relevant_indices");
        let named = name_id.and_then(|nid| index.by_name.get(&nid));
        MergedIndices {
            named: named.map(|v| v.as_slice()).unwrap_or(&[]),
            wildcards: &index.wildcards,
            named_pos: 0,
            wildcard_pos: 0,
        }
    }

    pub(crate) fn lookup(
        &self,
        name_id: Option<xot::NameId>,
        mut matches: impl FnMut(&Pattern<function::InlineFunctionId>) -> bool,
    ) -> Option<&V> {
        for idx in self.relevant_indices(name_id) {
            let (pattern, value) = &self.patterns[idx];
            if matches(pattern) {
                return Some(value);
            }
        }
        None
    }

    pub(crate) fn lookup_with_ambiguity(
        &self,
        name_id: Option<xot::NameId>,
        mut matches: impl FnMut(&Pattern<function::InlineFunctionId>) -> bool,
        same_rank: impl Fn(&V, &V) -> bool,
    ) -> Option<(&V, bool)> {
        let mut first = None;
        for idx in self.relevant_indices(name_id) {
            let (pattern, value) = &self.patterns[idx];
            if !matches(pattern) {
                continue;
            }
            if let Some(first_value) = first {
                return Some((first_value, same_rank(first_value, value)));
            }
            first = Some(value);
        }
        first.map(|value| (value, false))
    }

    pub(crate) fn lookup_after(
        &self,
        name_id: Option<xot::NameId>,
        mut matches: impl FnMut(&Pattern<function::InlineFunctionId>) -> bool,
        is_current: impl Fn(&V) -> bool,
    ) -> Option<&V> {
        let mut seen_current = false;
        for idx in self.relevant_indices(name_id) {
            let (pattern, value) = &self.patterns[idx];
            if !matches(pattern) {
                continue;
            }
            if !seen_current {
                if is_current(value) {
                    seen_current = true;
                }
                continue;
            }
            return Some(value);
        }
        None
    }

    pub(crate) fn lookup_after_lower_import_precedence(
        &self,
        name_id: Option<xot::NameId>,
        current_import_precedence: i64,
        mut matches: impl FnMut(&Pattern<function::InlineFunctionId>) -> bool,
        is_current: impl Fn(&V) -> bool,
        import_precedence_of: impl Fn(&V) -> i64,
        is_eligible: impl Fn(&V) -> bool,
    ) -> Option<&V> {
        let mut seen_current = false;
        for idx in self.relevant_indices(name_id) {
            let (pattern, value) = &self.patterns[idx];
            if !matches(pattern) {
                continue;
            }
            if !seen_current {
                if is_current(value) {
                    seen_current = true;
                }
                continue;
            }
            if import_precedence_of(value) < current_import_precedence && is_eligible(value) {
                return Some(value);
            }
        }
        None
    }
}

/// Iterator that merges two sorted index slices, yielding indices in
/// ascending order (which corresponds to priority order in the patterns Vec).
struct MergedIndices<'a> {
    named: &'a [usize],
    wildcards: &'a [usize],
    named_pos: usize,
    wildcard_pos: usize,
}

impl Iterator for MergedIndices<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        let n = self.named.get(self.named_pos);
        let w = self.wildcards.get(self.wildcard_pos);
        match (n, w) {
            (Some(&ni), Some(&wi)) => {
                if ni <= wi {
                    self.named_pos += 1;
                    // Skip duplicate if wildcard has the same index (union
                    // patterns can appear in both lists)
                    if ni == wi {
                        self.wildcard_pos += 1;
                    }
                    Some(ni)
                } else {
                    self.wildcard_pos += 1;
                    Some(wi)
                }
            }
            (Some(&ni), None) => {
                self.named_pos += 1;
                Some(ni)
            }
            (None, Some(&wi)) => {
                self.wildcard_pos += 1;
                Some(wi)
            }
            (None, None) => None,
        }
    }
}

/// Get the NameId of an item if it's a named node (element or attribute).
pub(crate) fn item_name_id(item: &Item, xot: &Xot) -> Option<xot::NameId> {
    if let Item::Node(node) = item {
        xot.node_name(*node)
    } else {
        None
    }
}

/// Resolve an OwnedName to a NameId via xot.
fn resolve_owned_name(name: &xot::xmlname::OwnedName, xot: &Xot) -> Option<xot::NameId> {
    let namespace_id = xot.namespace(name.namespace())?;
    let name_id = xot.name_ns(name.local_name(), namespace_id)?;
    Some(name_id)
}

/// The anchor name extracted from a pattern — used to classify patterns
/// for name-based indexing.
enum AnchorName<'a> {
    /// Pattern's final step tests a specific element/attribute name.
    Name(&'a xot::xmlname::OwnedName),
    /// Pattern can match any name (wildcard, kind-test, predicate-only, etc.).
    Wildcard,
    /// Union pattern with multiple specific names.
    Multiple(Vec<&'a xot::xmlname::OwnedName>),
}

/// Extract the anchor name from a pattern — the name test of the final step,
/// which is the step that must match the current node.
fn extract_anchor_name<E>(pattern: &Pattern<E>) -> AnchorName<'_> {
    match pattern {
        Pattern::Predicate(_) => AnchorName::Wildcard,
        Pattern::Expr(expr) => extract_anchor_from_expr(expr),
    }
}

fn extract_anchor_from_expr<E>(expr: &pattern::ExprPattern<E>) -> AnchorName<'_> {
    match expr {
        pattern::ExprPattern::Path(path) => extract_anchor_from_path(path),
        pattern::ExprPattern::BinaryExpr(binary) => extract_anchor_from_binary(binary),
    }
}

fn extract_anchor_from_path<E>(path: &pattern::PathExpr<E>) -> AnchorName<'_> {
    // The last step matches the current node
    if let Some(last_step) = path.steps.last() {
        match last_step {
            pattern::StepExpr::AxisStep(axis_step) => {
                extract_anchor_from_node_test(&axis_step.node_test)
            }
            pattern::StepExpr::PostfixExpr(postfix) => extract_anchor_from_expr(&postfix.expr),
        }
    } else {
        // No steps — matches document node (e.g., pattern "/")
        AnchorName::Wildcard
    }
}

fn extract_anchor_from_node_test(node_test: &ast::NodeTest) -> AnchorName<'_> {
    match node_test {
        ast::NodeTest::NameTest(name_test) => match name_test {
            ast::NameTest::Name(name_s) => AnchorName::Name(&name_s.value),
            _ => AnchorName::Wildcard, // Star, LocalName, Namespace
        },
        ast::NodeTest::KindTest(_) => AnchorName::Wildcard,
    }
}

fn extract_anchor_from_binary<E>(binary: &pattern::BinaryExpr<E>) -> AnchorName<'_> {
    match binary.operator {
        pattern::Operator::Union => {
            // Collect anchor names from both branches
            let left = extract_anchor_from_expr(&binary.left);
            let right = extract_anchor_from_expr(&binary.right);
            match (left, right) {
                (AnchorName::Name(a), AnchorName::Name(b)) => {
                    AnchorName::Multiple(vec![a, b])
                }
                (AnchorName::Name(a), AnchorName::Multiple(mut v))
                | (AnchorName::Multiple(mut v), AnchorName::Name(a)) => {
                    v.push(a);
                    AnchorName::Multiple(v)
                }
                (AnchorName::Multiple(mut a), AnchorName::Multiple(b)) => {
                    a.extend(b);
                    AnchorName::Multiple(a)
                }
                // If either branch is a wildcard, the whole union is a wildcard
                _ => AnchorName::Wildcard,
            }
        }
        // Intersect/except: use the left branch for indexing
        pattern::Operator::Intersect | pattern::Operator::Except => {
            extract_anchor_from_expr(&binary.left)
        }
    }
}

/// Walk a pattern and resolve all NameTest::Name OwnedNames to NameId,
/// storing them in `cache` keyed by pointer-as-integer.
fn collect_pattern_names<E>(
    pattern: &Pattern<E>,
    xot: &Xot,
    cache: &mut NameCache,
) {
    match pattern {
        Pattern::Predicate(_) => {}
        Pattern::Expr(expr) => collect_expr_names(expr, xot, cache),
    }
}

fn collect_expr_names<E>(
    expr: &pattern::ExprPattern<E>,
    xot: &Xot,
    cache: &mut NameCache,
) {
    match expr {
        pattern::ExprPattern::Path(path) => {
            for step in &path.steps {
                collect_step_names(step, xot, cache);
            }
        }
        pattern::ExprPattern::BinaryExpr(binary) => {
            collect_expr_names(&binary.left, xot, cache);
            collect_expr_names(&binary.right, xot, cache);
        }
    }
}

fn collect_step_names<E>(
    step: &pattern::StepExpr<E>,
    xot: &Xot,
    cache: &mut NameCache,
) {
    match step {
        pattern::StepExpr::AxisStep(axis_step) => {
            collect_node_test_names(&axis_step.node_test, xot, cache);
        }
        pattern::StepExpr::PostfixExpr(postfix) => {
            collect_expr_names(&postfix.expr, xot, cache);
        }
    }
}

fn collect_node_test_names(
    node_test: &ast::NodeTest,
    xot: &Xot,
    cache: &mut NameCache,
) {
    if let ast::NodeTest::NameTest(ast::NameTest::Name(name_s)) = node_test {
        let key = &name_s.value as *const _ as usize;
        if !cache.contains_key(&key) {
            if let Some(name_id) = resolve_owned_name(&name_s.value, xot) {
                cache.insert(key, name_id);
            }
            // Names that can't be resolved now (e.g. from documents not yet
            // loaded via doc()) are NOT cached — they fall back to direct
            // resolution at match time.
        }
    }
}
